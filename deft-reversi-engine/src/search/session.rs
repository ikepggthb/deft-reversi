//! 1 回の探索セッション全体で共有するランタイム状態（[`Runtime`]）と、
//! スレッドローカルな探索コンテキスト（[`Context`]）、中断制御（[`Control`]）を定義するモジュールです。
use super::*;

/// 全探索スレッドが共有する停止・制限・予算管理状態です。
pub(super) struct Control {
    /// 最初の停止理由です（[`StopReason`] の `u8` 表現、0 は未停止）。
    pub(super) reason: AtomicU8,
    pub(super) stop: StopHandle,
    pub(super) deadline: Option<Instant>,
    pub(super) node_limit: Option<u64>,
    /// ノード予算をチャンク単位で払い出すためのカウンタです（偽共有防止のためパディングしています）。
    pub(super) issued: Padded<AtomicU64>,
}
impl Control {
    /// 最初の停止理由を記録します（2 回目以降は先着理由を保持します）。
    pub(super) fn set(&self, reason: StopReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Relaxed);
    }
    /// ワーカーのパニックなどによる失敗を記録します（結果は破棄されるため他の停止理由を上書きします）。
    pub(super) fn fail(&self) {
        self.reason
            .store(StopReason::WorkerFailed as u8, Ordering::Release);
    }
    /// 外部停止要求または時間制限に達していないかを確認し、達していれば停止状態へ遷移させます。
    pub(super) fn check(&self) -> bool {
        if self.stop.is_stopped() {
            self.set(StopReason::Requested)
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.set(StopReason::TimeLimit)
        }
        self.reason.load(Ordering::Acquire) != 0
    }
    /// 記録された [`StopReason`] を返します。
    pub(super) fn reason(&self) -> StopReason {
        [
            StopReason::Completed,
            StopReason::Requested,
            StopReason::TimeLimit,
            StopReason::NodeLimit,
            StopReason::WorkerFailed,
        ][usize::from(self.reason.load(Ordering::Acquire))]
    }
}
/// 全探索ノードで各スレッドが参照する共有ランタイム状態です。
/// 探索中に書き換えられるフィールドは [`Padded`] で分離し、読み取り専用フィールドとの偽共有（キャッシュライン競合）を防ぎます。
#[repr(align(128))]
pub(super) struct Runtime {
    pub(super) model: Arc<PatternModel>,
    pub(super) table: Arc<Table>,
    pub(super) pv_table: Arc<Table>,
    pub(super) pool: Arc<Pool>,
    pub(super) control: Control,
    pub(super) generation: u8,
    pub(super) stats: Padded<Mutex<SearchStats>>,
    pub(super) pending: Padded<AtomicUsize>,
    pub(super) pending_lock: Mutex<()>,
    pub(super) pending_ready: Padded<std::sync::Condvar>,
    pub(super) parallel: bool,
    /// 終盤完全読みにおいてノード分割（YBWC による並列化）を許可する最小空きマス数（`tree::split_empties`）です。
    pub(super) split_empties: u8,
    /// 実行中イテレーションの選択率タグ（`Selectivity::quality`）です。0 なら ProbCut なしの全幅探索を行います。
    pub(super) quality: AtomicU8,
    pub(super) rules: Arc<[Option<ProbCutRule>]>,
    /// 各選択率ステージにおける偏差倍率（σ 倍率）です。
    pub(super) sigmas: ProbCutSigmas,
    /// ローカル終盤ソルバが ProbCut を試みる最小空きマス数です。
    pub(super) endgame_probcut_min: u8,
    /// スレッドローカル終盤キャッシュを再利用できるエポック番号です。
    pub(super) cache_epoch: u64,
}
impl Runtime {
    /// 実行中のイテレーションで ProbCut 枝刈りを行うかを返します。
    pub(super) fn selective(&self) -> bool {
        self.quality.load(Ordering::Relaxed) != 0
    }
    /// イテレーションの選択率を設定します。前イテレーションの全ワーカー完了後にのみ呼び出してください。
    pub(super) fn set_stage(&self, selectivity: Selectivity) {
        self.quality.store(selectivity.quality(), Ordering::Relaxed);
    }

    /// `pending` に計上されたヘルパージョブを 1 つ完了させます。
    /// 最後の 1 つが完了したときは、`drain` が状態確認と待機の間で保持するロック下で通知することで
    /// 通知の取りこぼし（lost wakeup）を防ぎます。
    pub(super) fn finish_pending(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _guard = parallel::lock(&self.pending_lock);
            self.pending_ready.notify_all();
        }
    }
    /// 投入済みのすべてのヘルパージョブが完了するまで待機します。
    pub(super) fn drain(&self) {
        let mut guard = parallel::lock(&self.pending_lock);
        while self.pending.load(Ordering::Acquire) != 0 {
            let (next, _) = self
                .pending_ready
                .wait_timeout(guard, Duration::from_millis(5))
                .unwrap_or_else(|p| p.into_inner());
            guard = next;
        }
    }
}
/// 停止要求・制限到達・ワーカー失敗・祖先分割でのベータカット（より良い手が既に確定したことによる枝刈り）により
/// 探索が中断されたことを示すマーカーです。中断された部分結果は使用してはなりません。
#[derive(Debug)]
pub(super) struct Aborted;
/// 探索から返される評価値と最善応手手順（PV）です。
#[derive(Clone)]
pub(super) struct Line {
    pub(super) score: i32,
    pub(super) actions: [Option<Action>; 128],
    pub(super) len: usize,
}
impl Line {
    /// 手順が空の葉ノード結果を生成します。
    pub(super) fn leaf(score: i32) -> Self {
        Self {
            score,
            actions: [None; 128],
            len: 0,
        }
    }
    /// 子ノードの `Line` の先頭に `action` を付加し、手番反転のためスコア符号を反転します。
    pub(super) fn prepend(action: Action, child: &Self) -> Self {
        let mut line = Self::leaf(-child.score);
        line.actions[0] = Some(action);
        line.len = (child.len + 1).min(128);
        line.actions[1..line.len].copy_from_slice(&child.actions[..line.len - 1]);
        line
    }
    /// 保持している手順を `Vec<Action>` として取り出します。
    pub(super) fn moves(&self) -> Vec<Action> {
        self.actions[..self.len].iter().flatten().copied().collect()
    }
}
/// ビットボードと差分パターン評価状態（[`EvalState`]）をまとめた探索用局面表現です。
#[derive(Clone, Copy)]
pub(super) struct Position {
    pub(super) board: Board,
    pub(super) state: EvalState,
}
/// オーダリング済みの 1 合法手候補です。
#[derive(Clone, Copy)]
pub(super) struct Candidate {
    pub(super) square: Square,
    pub(super) flips: u64,
    pub(super) ordering: i32,
}
impl Candidate {
    /// 候補手を適用した子局面を生成します。`update_eval` が false のときは `EvalState` 更新を省略します。
    pub(super) fn child(self, parent: Position, update_eval: bool) -> Position {
        let bit = self.square.bit();
        Position {
            board: parent.board.after_known_move(bit, self.flips),
            state: if update_eval {
                parent.state.played(bit, self.flips)
            } else {
                parent.state
            },
        }
    }
    /// 候補手を [`Action`] に変換します。
    pub(super) fn action(self) -> Action {
        Action::Place(self.square)
    }
}
/// 各探索スレッド（およびヘルパージョブ）が保持するローカル探索コンテキストです。
/// Drop 時にローカル統計を [`Runtime::stats`] へマージします。
pub(super) struct Context {
    pub(super) runtime: Arc<Runtime>,
    pub(super) stats: SearchStats,
    pub(super) ancestors: Vec<Arc<Helper>>,
    pub(super) cuts: Vec<Arc<Padded<AtomicBool>>>,
    pub(super) end_cache: EndgameCache,
    #[cfg(feature = "endgame-pattern-ordering")]
    pub(super) end_ordering: [Option<(Board, EvalState)>; 3],
}
impl Drop for Context {
    fn drop(&mut self) {
        parallel::lock(&self.runtime.stats).merge(&self.stats)
    }
}

/// 探索ノードの探索仕様（深さ・完全読みフラグ・ProbCut 許可フラグ）です。
#[derive(Clone, Copy)]
pub(super) struct SearchSpec {
    pub(super) depth: u8,
    pub(super) exact: bool,
    pub(super) probcut: bool,
}

#[cfg(test)]
pub(super) fn test_context() -> Context {
    let workers = Workers::new(0).unwrap();
    let runtime = Arc::new(Runtime {
        model: Arc::new(PatternModel::zero()),
        table: Arc::new(Table::new(1).unwrap()),
        pv_table: Arc::new(Table::new(1).unwrap()),
        pool: workers.pool.clone(),
        control: Control {
            reason: AtomicU8::new(0),
            stop: StopHandle::new(),
            deadline: None,
            node_limit: None,
            issued: Padded(AtomicU64::new(0)),
        },
        generation: 1,
        stats: Padded(Mutex::new(SearchStats::default())),
        pending: Padded(AtomicUsize::new(0)),
        pending_lock: Mutex::new(()),
        pending_ready: Padded(std::sync::Condvar::new()),
        parallel: false,
        split_empties: u8::MAX,
        quality: AtomicU8::new(0),
        rules: Arc::from([]),
        sigmas: ProbCutSigmas::NORMAL,
        endgame_probcut_min: super::endgame::PROBCUT_MIN_EMPTIES,
        cache_epoch: 0,
    });
    Context::new(runtime, Vec::new(), Vec::new())
}
