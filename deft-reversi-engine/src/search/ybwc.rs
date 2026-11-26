//! YBWC（Young Brothers Wait Concept）による並列探索の分割点（[`Split`]）を実装するモジュールです。
//!
//! 最初の有力な手（長男）を探索し終えて探索窓を狭めたあと、残りの兄弟候補を待機中の祖先スレッドや
//! 空いているワーカースレッドへ分配して並列に探索します。
//! もし途中でベータカット（これ以上調べる必要がないほどの良い手が見つかること）が発生したら、
//! 残りの兄弟ジョブを即座に中断して合流します。
use super::tree::pick_best;
use super::*;

impl Context {
    /// 現在のノードを分割点（[`Split`]）として残りの候補手を並列探索します。
    pub(super) fn split(
        &mut self,
        position: Position,
        candidates: Vec<Candidate>,
        window: (i32, i32),
        spec: SearchSpec,
        best: Line,
        root_bounds: Option<Arc<Mutex<[root::RootBound; 64]>>>,
    ) -> Result<Line, Aborted> {
        let (alpha, beta) = window;
        let SearchSpec {
            depth,
            exact,
            probcut: allow_probcut,
        } = spec;
        let cut = Arc::new(Padded(AtomicBool::new(false)));
        let helper = Arc::new(Helper::new());
        let mut ancestors = self.ancestors.clone();
        ancestors.push(helper.clone());
        let mut cuts = self.cuts.clone();
        cuts.push(cut.clone());
        let point = Arc::new(Split {
            position,
            root_bounds,
            state: Mutex::new(SplitState {
                candidates,
                next: 0,
                alpha,
                best,
            }),
            beta,
            depth,
            exact,
            allow_probcut,
            cut,
            helper,
            active: AtomicUsize::new(0),
            ancestors,
            cuts,
        });
        // パニックによるスタックの巻き戻しが起きた場合でも、割り当て済みの子孫ヘルパージョブが完了するまで必ず待機します。
        // 待機せずに親スレッドが抜けてしまうと、親に仕事を引き渡そうとしていた子孫ジョブが
        // 実行者不在のまま取り残される恐れがあるためです。
        let _join_on_unwind = JoinOnUnwind {
            point: point.clone(),
            runtime: self.runtime.clone(),
        };
        // マスター自身の 1 手を除いた残りの候補手数分だけヘルパーを募集します。
        // 空いているスレッドだけが引き受けるため、余剰スレッド数に応じた自然な並列度になります。
        let wanted = parallel::lock(&point.state).candidates.len() - 1;
        let spawned = self.offer_helpers(&point, wanted);
        if spawned != 0 {
            self.stats.splits += 1;
        }
        // マスター自身も候補手を探索し、その後合流待ちの間は自分の配下の子孫ジョブだけを受け入れて手伝います（デッドロック防止）。
        let old_ancestors = std::mem::replace(&mut self.ancestors, point.ancestors.clone());
        let old_cuts = std::mem::replace(&mut self.cuts, point.cuts.clone());
        self.split_loop(&point);
        let join_start = Instant::now();
        // 合流待ち中に実行する子孫ジョブとスレッドローカル終盤キャッシュを共有するため、一時的にスレッド枠へ返却します。
        let epoch = self.runtime.cache_epoch;
        std::mem::take(&mut self.end_cache).put_thread(epoch);
        point
            .helper
            .wait(|| point.active.load(Ordering::Acquire) == 0);
        self.end_cache = EndgameCache::take_thread(epoch);
        self.stats.join_time += join_start.elapsed();
        self.ancestors = old_ancestors;
        self.cuts = old_cuts;
        if self.aborted() {
            return Err(Aborted);
        }
        let result = parallel::lock(&point.state).best.clone();
        Ok(result)
    }
    /// 分割点 `point` に対して最大 `wanted` 個のヘルパージョブを空きスレッドへ提供し、引き受けられた数を返します。
    fn offer_helpers(&mut self, point: &Arc<Split>, wanted: usize) -> usize {
        let mut spawned = 0;
        for _ in 0..wanted {
            let runtime = self.runtime.clone();
            let task_point = point.clone();
            // 募集側のスレッド自身（join 前のマスター、または active に計上中のヘルパー）がまだ動作しているため、
            // 途中で `active` が 0 になることはありません。
            point.active.fetch_add(1, Ordering::AcqRel);
            self.runtime.pending.fetch_add(1, Ordering::AcqRel);
            let job: parallel::Job = Box::new(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut context = Context::new(
                        runtime.clone(),
                        task_point.ancestors.clone(),
                        task_point.cuts.clone(),
                    );
                    context.end_cache = EndgameCache::take_thread(runtime.cache_epoch);
                    context.stats.helper_jobs += 1;
                    context.split_loop(&task_point);
                    std::mem::take(&mut context.end_cache).put_thread(runtime.cache_epoch);
                }));
                if result.is_err() {
                    runtime.control.fail();
                }
                task_point.active.fetch_sub(1, Ordering::AcqRel);
                task_point.helper.wake();
                runtime.finish_pending();
            });
            match self.runtime.pool.offer(job, &self.ancestors) {
                Ok(dispatch) => {
                    spawned += 1;
                    match dispatch {
                        parallel::Dispatch::Ancestor => self.stats.ancestor_handoffs += 1,
                        parallel::Dispatch::Worker => self.stats.worker_dispatches += 1,
                    }
                }
                Err(_) => {
                    point.active.fetch_sub(1, Ordering::AcqRel);
                    self.runtime.pending.fetch_sub(1, Ordering::AcqRel);
                    break;
                }
            }
        }
        spawned
    }
    /// 分割点から未探索の候補手を 1 手ずつ取り出して探索し、最善手・α値・ベータカットを更新するループです。
    fn split_loop(&mut self, point: &Arc<Split>) {
        loop {
            if self.aborted() {
                break;
            }
            let next = {
                let mut state = parallel::lock(&point.state);
                if state.next == state.candidates.len() || state.alpha >= point.beta {
                    None
                } else {
                    let index = state.next;
                    pick_best(&mut state.candidates, index);
                    let candidate = state.candidates[index];
                    state.next += 1;
                    Some((candidate, state.alpha))
                }
            };
            let Some((candidate, alpha)) = next else {
                break;
            };
            let Ok(line) = self.sibling(
                point.position,
                candidate,
                alpha,
                point.beta,
                SearchSpec {
                    depth: point.depth,
                    exact: point.exact,
                    probcut: point.allow_probcut,
                },
            ) else {
                break;
            };
            if let Some(bounds) = &point.root_bounds {
                parallel::lock(bounds)[candidate.square.index() as usize]
                    .update(line.score, alpha, point.beta);
            }
            let mut state = parallel::lock(&point.state);
            if line.score > state.best.score {
                state.best = line;
            }
            state.alpha = state.alpha.max(state.best.score);
            if state.alpha >= point.beta {
                point.cut.store(true, Ordering::Release);
                self.stats.beta_cuts += 1;
                point.helper.wake();
                break;
            }
            // 分割後に他のスレッドが空きになった場合も、残り候補手を引き受けられるよう再度募集します。
            let remaining = state.candidates.len() - state.next;
            drop(state);
            if remaining >= 2 && self.runtime.pool.can_offer(&self.ancestors) {
                self.offer_helpers(point, remaining - 1);
            }
        }
    }
}
/// 分割点のミューテックスで保護された動的共有状態です。
struct SplitState {
    candidates: Vec<Candidate>,
    next: usize,
    alpha: i32,
    best: Line,
}
// すべてのヘルパージョブに `Arc` がクローンされるため、参照カウントの更新が共有状態と同じ
// キャッシュラインに乗らないよう 128 バイト境界に揃えています。
#[repr(align(128))]
/// YBWC の 1 つの並列分割点です。
struct Split {
    position: Position,
    root_bounds: Option<Arc<Mutex<[root::RootBound; 64]>>>,
    state: Mutex<SplitState>,
    beta: i32,
    depth: u8,
    exact: bool,
    allow_probcut: bool,
    cut: Arc<Padded<AtomicBool>>,
    helper: Arc<Helper>,
    active: AtomicUsize,
    /// この分割点のヘルパージョブが引き継ぐリストです（末尾が自身の `helper` と `cut`）。
    ancestors: Vec<Arc<Helper>>,
    cuts: Vec<Arc<Padded<AtomicBool>>>,
}

/// パニックによるスタック巻き戻し時に子孫ヘルパージョブの完了を待機する RAII ガードです。
struct JoinOnUnwind {
    point: Arc<Split>,
    runtime: Arc<Runtime>,
}
impl Drop for JoinOnUnwind {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.runtime.control.fail();
            self.point.cut.store(true, Ordering::Release);
            self.point
                .helper
                .wait(|| self.point.active.load(Ordering::Acquire) == 0);
        }
    }
}
