//! 探索エンジン本体（[`Engine`]）と、反復深化スケジューラ・置換表からのウォームスタート・PV（最善応手列）復元を制御するモジュールです。

use crate::{
    Action, Board, Model, Square,
    evaluation::{EvalState, PatternModel},
    parallel::{self, Helper, Padded, Pool, Workers},
    transposition::{Entry, Table},
};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

mod api;
pub use api::*;
mod session;
use session::*;
mod endgame;
use endgame::{EndgameCache, new_cache_epoch};
mod level;
pub use level::level_reading;
mod ordering;
mod pv;
mod root;
mod tree;
mod ybwc;

/// 評価モデル・置換表（TT）・ワーカースレッドプールを所有する探索エンジンです。
/// 1 インスタンスにつき同時に 1 つの探索を実行できます（非同期実行には [`EngineWorker`](crate::EngineWorker) を使います）。
pub struct Engine {
    model: Arc<PatternModel>,
    table: Arc<Table>,
    pv_table: Arc<Table>,
    workers: Workers,
    options: EngineOptions,
    generation: u8,
    calibration: Arc<[Option<ProbCutRule>]>,
    sigmas: ProbCutSigmas,
    end_cache: EndgameCache,
    /// 現在のモデル・較正・置換表状態に対して有効なローカル終盤キャッシュを識別するエポック番号です。
    cache_epoch: u64,
}
impl Engine {
    /// 指定されたモデルとオプション（スレッド数・置換表サイズ）で [`Engine`] を構築します。
    pub fn new(model: Model, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        let (table, pv_table) = new_tables(options.tt_mib)?;
        let workers = Workers::new(options.threads - 1).map_err(EngineError::Thread)?;
        let (rules, sigmas) = split_calibration(model.calibration);
        Ok(Self {
            model: Arc::new(model.pattern),
            calibration: indexed_rules(rules),
            sigmas,
            table,
            pv_table,
            workers,
            options,
            generation: 0,
            end_cache: EndgameCache::default(),
            cache_epoch: new_cache_epoch(),
        })
    }
    /// 不変な評価モデル・ProbCut 較正データを共有しつつ、独立した置換表とワーカースレッドを持つ
    /// 新しい [`Engine`] インスタンスを複製します。バッチ処理用に独立したワーカープールを構築したい場合に便利です。
    pub fn fork(&self, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        let (table, pv_table) = new_tables(options.tt_mib)?;
        Ok(Self {
            model: self.model.clone(),
            calibration: self.calibration.clone(),
            sigmas: self.sigmas,
            table,
            pv_table,
            workers: Workers::new(options.threads - 1).map_err(EngineError::Thread)?,
            options,
            generation: 0,
            end_cache: EndgameCache::default(),
            cache_epoch: new_cache_epoch(),
        })
    }
    /// 待機中のエンジンのスレッド数と置換表サイズを再構成します。
    /// 置換表サイズが変わらない場合は既存のテーブルを保持し、スレッド数変更時は旧ワーカーを join してから再生成します。
    pub fn set_resources(&mut self, options: EngineOptions) -> Result<(), EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        if options.tt_mib != self.options.tt_mib {
            (self.table, self.pv_table) = new_tables(options.tt_mib)?;
            self.invalidate_end_caches();
            self.options.tt_mib = options.tt_mib;
        }
        if options.threads != self.options.threads {
            self.workers = Workers::new(0).map_err(EngineError::Thread)?;
            self.options.threads = 1;
            self.workers = Workers::new(options.threads - 1).map_err(EngineError::Thread)?;
        }
        self.options = options;
        Ok(())
    }
    /// 現在の [`EngineOptions`] を返します。
    pub fn options(&self) -> EngineOptions {
        self.options
    }
    /// 評価モデルと ProbCut 較正データを差し替え、すべての置換表をクリアします。
    pub fn set_model(&mut self, model: Model) -> Result<(), EngineError> {
        let (rules, sigmas) = split_calibration(model.calibration);
        self.set_rules(rules, sigmas)?;
        self.model = Arc::new(model.pattern);
        Ok(())
    }
    /// すべての置換表とローカル終盤キャッシュをクリアします（新規対局の開始時など）。
    pub fn clear_tt(&mut self) -> Result<(), EngineError> {
        (self.table, self.pv_table) = new_tables(self.options.tt_mib)?;
        self.invalidate_end_caches();
        Ok(())
    }
    /// 探索を実行せずに、指定局面とリクエストが解決する目標深さ・モード・選択率を返します。
    /// ここでの検証はリクエストの妥当性だけを確認し、MPC 較正の有無は実際の選択探索開始時に検査します。
    pub fn search_plan(
        &self,
        position: impl Into<Board>,
        request: &SearchRequest,
    ) -> Result<ResultKind, EngineError> {
        let job = self.resolve(position.into(), request)?;
        Ok(match job.mode {
            SearchMode::Depth(depth) => ResultKind::Depth {
                depth: depth.min(job.board.empty_count()),
                selectivity: job.selectivity,
            },
            SearchMode::Exact | SearchMode::SelectiveEndgame => ResultKind::Endgame {
                selectivity: job.selectivity,
            },
        })
    }
    /// 呼び出し元スレッドをブロックして `position` を探索します。
    pub fn search(
        &mut self,
        position: impl Into<Board>,
        request: &SearchRequest,
    ) -> Result<SearchResult, EngineError> {
        self.search_with(position, request, |_| {})
    }
    /// `search` と同様ですが、反復深化の各段階が終わるたびに呼び出し元スレッド上で `on_progress` を呼び出します。
    pub fn search_with(
        &mut self,
        position: impl Into<Board>,
        request: &SearchRequest,
        mut on_progress: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let job = self.resolve(position.into(), request)?;
        if job.selectivity != Selectivity::Full && self.calibration.is_empty() {
            return Err(EngineError::InvalidCalibration(
                "ProbCut requires model-specific calibration",
            ));
        }
        self.run(job, &mut on_progress)
    }
    /// 旧メインブランチ互換の MPC テーブルをインストールします（`data/eval/eval.json` 変換モデル用の明示的オプトインです）。
    #[cfg(feature = "tuning")]
    pub fn set_legacy_main_mpc(&mut self) -> Result<(), EngineError> {
        self.set_rules(crate::mpc::legacy_main_rules(), ProbCutSigmas::NORMAL)
    }
    /// 正規分布の偏差倍率で ProbCut ルールをインストールします。
    #[cfg(feature = "tuning")]
    pub fn set_probcut_rules(&mut self, rules: Vec<ProbCutRule>) -> Result<(), EngineError> {
        self.set_probcut_calibration(rules, ProbCutSigmas::NORMAL)
    }
    /// 実測した各ステージの偏差倍率と合わせて ProbCut ルールをインストールします。
    #[cfg(feature = "tuning")]
    pub fn set_probcut_calibration(
        &mut self,
        rules: Vec<ProbCutRule>,
        sigmas: ProbCutSigmas,
    ) -> Result<(), EngineError> {
        validate_calibration(&rules, &sigmas)?;
        self.set_rules(rules, sigmas)
    }
    /// 新しい ProbCut ルールを適用します。既存のキャッシュ結果は旧ルールに依存しているため、先に置換表をクリアします。
    fn set_rules(
        &mut self,
        rules: Vec<ProbCutRule>,
        sigmas: ProbCutSigmas,
    ) -> Result<(), EngineError> {
        self.clear_tt()?;
        self.calibration = indexed_rules(rules);
        self.sigmas = sigmas;
        Ok(())
    }
    /// メインスレッドのローカル終盤キャッシュを破棄し、新しいエポック番号を発行して全ワーカーのキャッシュも無効化します。
    fn invalidate_end_caches(&mut self) {
        self.end_cache.clear();
        self.cache_epoch = new_cache_epoch();
    }
    /// リクエストを検証し、ルート局面に対する実行ジョブ [`Job`] へ解決します。
    fn resolve(&self, board: Board, request: &SearchRequest) -> Result<Job, EngineError> {
        let (mode, selectivity, level) = match request.goal {
            Goal::Depth(depth) if depth > 60 => {
                return Err(EngineError::InvalidRequest("depth must be 0..=60"));
            }
            Goal::Depth(depth) => (
                SearchMode::Depth(depth),
                request.selectivity.unwrap_or_default(),
                None,
            ),
            Goal::Endgame => {
                let selectivity = request.selectivity.unwrap_or_default();
                let mode = if selectivity == Selectivity::Full {
                    SearchMode::Exact
                } else {
                    SearchMode::SelectiveEndgame
                };
                (mode, selectivity, None)
            }
            Goal::Level(_) | Goal::Plan(_) => {
                let plan = match request.goal {
                    Goal::Plan(plan) => plan,
                    Goal::Level(level) => SearchPlan::level(level)?,
                    _ => unreachable!(),
                };
                plan.validate()?;
                if request.selectivity.is_some() {
                    return Err(EngineError::InvalidRequest(
                        "a level or plan chooses its own selectivity",
                    ));
                }
                let (mode, selectivity) = plan.at(board.empty_count());
                (mode, selectivity, Some(plan.midgame_depth()))
            }
        };
        if let Some(mask) = request.root_moves {
            let legal = board.get_moves();
            if mask & !legal != 0 || (legal != 0 && mask == 0) {
                return Err(EngineError::InvalidRequest(
                    "root move mask must be a nonempty legal subset",
                ));
            }
        }
        if request.review_move.is_some_and(|square| {
            request.root_moves.unwrap_or_else(|| board.get_moves()) & square.bit() == 0
        }) {
            return Err(EngineError::InvalidRequest(
                "review move must be in the legal root mask",
            ));
        }
        if request.minimum_score.is_some_and(|score| {
            !(-63..=64).contains(&score)
                || request.all_moves
                || request.review_move.is_some()
                || board.get_moves() == 0
                || !matches!(mode, SearchMode::Depth(d) if d > 0 && d < board.empty_count())
        }) {
            return Err(EngineError::InvalidRequest(
                "minimum score requires a positive midgame depth and legal root moves",
            ));
        }
        Ok(Job {
            minimum_score: request.minimum_score,
            reuse_pv: request.reuse_pv,
            keep_generation: request.keep_generation,
            recover_endgame_pv: request.recover_endgame_pv,
            recover_pv: request.recover_pv,
            skip_pv_recovery: request.skip_pv_recovery,
            root_moves: request.root_moves,
            preferred_move: request.preferred_move,
            board,
            mode,
            selectivity,
            level,
            time_limit: request.time_limit,
            timed_levels: request.timed_levels && !self.calibration.is_empty(),
            node_limit: request.node_limit,
            all_moves: request.all_moves,
            move_pvs: request.move_pvs,
            review_move: request.review_move,
            stop: request.stop.clone(),
        })
    }
    /// 探索ドライバー本体です（反復深化・PV 復元・全手評価を実行します）。
    fn run(
        &mut self,
        job: Job,
        on_progress: &mut dyn FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        // ルート局面が強制パスの場合は、探索ドライバーを起動する前に 1 手パスした局面へ委譲して結果を反転します。
        if job.board.must_pass() {
            let pass_start = Instant::now();
            let all_moves = job.all_moves;
            let move_pvs = job.move_pvs;
            let passed = Job {
                board: job.board.board_after_pass(),
                all_moves: false,
                move_pvs: false,
                root_moves: None,
                preferred_move: None,
                ..job
            };
            let mut forward = |progress: &Progress| {
                let mut progress = progress.clone();
                progress.score = -progress.score;
                progress.best_move = Some(Action::Pass);
                if searched(progress.kind) {
                    progress.pv.insert(0, Action::Pass);
                }
                on_progress(&progress);
            };
            let mut result = self.run(passed, &mut forward)?;
            result.score = result.score.map(|score| -score);
            result.warm_score = result.warm_score.map(|score| -score);
            result.best_move = Some(Action::Pass);
            if result.kind.is_some_and(searched) {
                result.pv.insert(0, Action::Pass);
            }
            if all_moves
                && result.stop_reason == StopReason::Completed
                && let Some(score) = result.score
            {
                result.moves = vec![MoveScore {
                    action: Action::Pass,
                    score,
                }];
                if move_pvs {
                    let pv = if result.pv.is_empty() {
                        vec![Action::Pass]
                    } else {
                        result.pv.clone()
                    };
                    result.move_pvs.push(pv);
                }
            }
            result.root_moves = 0;
            result.root_restricted = false;
            result.elapsed = pass_start.elapsed();
            return Ok(result);
        }
        // 各 TT エントリは自身の品質タグ (`quality`) と完全読みモードを保持しており、
        // 同一条件のエントリだけが枝刈りに使われます。そのため異なるステージ間でも置換表をクリアせずに維持できます
        // （較正パラメータが変更された場合だけクリアします）。
        if !job.keep_generation {
            self.generation = self.generation.wrapping_add(1);
        }
        let start = Instant::now();
        let deadline = match job.time_limit {
            Some(d) => Some(
                start
                    .checked_add(d)
                    .ok_or(EngineError::InvalidRequest("time limit overflow"))?,
            ),
            None => None,
        };
        let board = job.board;
        let mut result = initial_result(board);
        let mask = job.root_moves.unwrap_or_else(|| board.get_moves());
        result.root_moves = mask;
        result.root_restricted = mask != board.get_moves();
        if mask != 0 {
            result.best_move = Some(Action::Place(Square::lowest(mask)));
        }
        if result.best_move.is_none() {
            result.score = Some(board.final_score());
            result.kind = Some(ResultKind::Endgame {
                selectivity: Selectivity::Full,
            });
            result.elapsed = start.elapsed();
            return Ok(result);
        }
        let runtime = self.runtime(&job, deadline);
        let state = EvalState::from_board(board);
        let mut root = root::RootSearch::new(Position { board, state });
        root.restrict(mask, job.preferred_move);
        let mut context = Context::new(runtime.clone(), Vec::new(), Vec::new());
        // 探索中はこのコンテキストが `end_cache` を所有し、リクエスト終了後に Engine へ戻します。
        context.end_cache = std::mem::take(&mut self.end_cache);
        let mut last = None;
        let plan = self.plan(&job);
        let warm = self.warm_start(&job, mask, &plan);
        let (skip, mut prediction) = if let Some((skip, square, score)) = warm {
            // キャッシュされていない最初の反復中に時間切れになった場合、
            // 最小合法マスよりも互換性のあるキャッシュ済みの手のほうがよい暫定手になります。
            result.best_move = Some(Action::Place(square));
            result.warm_score = Some(score);
            root.restrict(mask, Some(square));
            context.stats.reused_iterations += skip as u64;
            (skip, score)
        } else {
            (0, self.model.evaluate(&board))
        };
        for (index, &iteration) in plan.iter().enumerate().skip(skip) {
            runtime.drain();
            runtime.set_stage(iteration.selectivity);
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if index + 1 == plan.len()
                    && let Some(minimum) = job.minimum_score
                    && root.below_minimum(&mut context, iteration.spec(), minimum)?
                {
                    return Ok((Line::leaf(minimum - 1), true));
                }
                root.iteration(&mut context, iteration.spec(), prediction)
                    .map(|line| (line, false))
            }));
            let completed = match attempt {
                Ok(Ok(line)) if !context.aborted() => Some(line),
                Ok(_) => None,
                Err(_) => {
                    runtime.control.fail();
                    None
                }
            };
            let Some((line, upper)) = completed else {
                break;
            };
            iteration.record(&mut result, &line);
            if upper {
                result.score_is_upper_bound = true;
                result.best_move = Some(Action::Place(Square::lowest(mask)));
                break;
            }
            prediction = line.score;
            last = Some(iteration);
            // ヘルパースレッドは完了時に自身のカウンタを合算します。
            runtime.drain();
            on_progress(&Progress {
                kind: result.kind.unwrap(),
                score: line.score,
                best_move: result.best_move,
                pv: result.pv.clone(),
                nodes: context.stats.nodes + parallel::lock(&runtime.stats).nodes,
                elapsed: start.elapsed(),
                next: plan.get(index + 1).map(|next| next.kind()),
                moves: Vec::new(),
            });
        }
        runtime.drain();
        // 置換表で枝刈り（カットオフ）が起きたときは再帰によって PV が構築されないため、
        // 確定スコアと一致する手を置換表や再探索から個別に辿って PV を復元します。
        if !job.skip_pv_recovery
            && !result.score_is_upper_bound
            && !context.aborted()
            && let Some(last) = last
            && (job.review_move.is_none() || job.all_moves)
            && last.depth > 0
            && let Some(score) = result.score
        {
            let full = last.selectivity == Selectivity::Full;
            let selective_recovery = if last.exact {
                (job.recover_endgame_pv || job.recover_pv)
                    && result.pv_final_score(board) != Some(score)
            } else {
                job.recover_pv && result.pv.len() < usize::from(last.depth)
            };
            if (full || selective_recovery)
                && let Ok(pv) =
                    context.extract_pv(board, score, last.depth, last.exact, result.best_move)
                && (full
                    || pv::terminal_score(board, &pv) == Some(score)
                    || (result.pv_final_score(board).is_none() && pv.len() > result.pv.len()))
            {
                result.pv = pv;
            }
        }
        runtime.drain();
        let score_mask = if job.all_moves {
            mask
        } else if let (Some(played), Some(Action::Place(best))) =
            (job.review_move, result.best_move)
        {
            played.bit() | best.bit()
        } else {
            0
        };
        if score_mask != 0
            && !context.aborted()
            && let Some(last) = last
            && last.depth > 0
            && let Ok(moves) = root.move_scores(
                &mut context,
                last.spec(),
                score_mask,
                job.move_pvs,
                &mut |context, moves| {
                    on_progress(&Progress {
                        kind: result.kind.unwrap(),
                        score: result.score.unwrap_or_default(),
                        best_move: result.best_move,
                        pv: result.pv.clone(),
                        nodes: context.stats.nodes + parallel::lock(&runtime.stats).nodes,
                        elapsed: start.elapsed(),
                        next: None,
                        moves: moves.to_vec(),
                    })
                },
            )
            && !context.aborted()
        {
            for (score, pv) in moves {
                result.moves.push(score);
                if job.move_pvs {
                    let pv = if result.best_move == Some(score.action)
                        && result.score == Some(score.score)
                        && result.pv.len() > pv.len()
                    {
                        result.pv.clone()
                    } else {
                        pv
                    };
                    result.move_pvs.push(pv);
                }
            }
        }
        runtime.drain();
        if runtime.control.reason() != StopReason::WorkerFailed {
            self.end_cache = std::mem::take(&mut context.end_cache);
        }
        drop(context);
        result.stop_reason = runtime.control.reason();
        result.stats = parallel::lock(&runtime.stats).clone();
        result.elapsed = start.elapsed();
        Ok(result)
    }
    /// 置換表に互換性のある完了済みイテレーションの結果があれば、初期の反復をスキップして
    /// 最善手と予測スコアをウォームスタートとして再利用します。
    fn warm_start(&self, job: &Job, mask: u64, plan: &[Iteration]) -> Option<(usize, Square, i32)> {
        if !job.reuse_pv || plan.len() < 2 {
            return None;
        }
        [self.pv_table.get(job.board), self.table.get(job.board)]
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let best = Square::new(entry.best)?;
                if mask & best.bit() == 0 || entry.lower != entry.upper {
                    return None;
                }
                // 同じ探索モードかつ同じ MPC ステージのイテレーションだけをスキップ対象とし、
                // より深い一致エントリがあっても目標（最終）イテレーション自体は必ず残します。
                let index = plan[..plan.len() - 1].iter().rposition(|iteration| {
                    entry.exact_mode == iteration.exact
                        && entry.quality == iteration.selectivity.quality()
                        && entry.depth >= iteration.depth
                })?;
                Some((index + 1, best, i32::from(entry.lower)))
            })
            .max_by_key(|(skip, _, _)| *skip)
    }
    fn runtime(&self, job: &Job, deadline: Option<Instant>) -> Arc<Runtime> {
        Arc::new(Runtime {
            model: self.model.clone(),
            table: self.table.clone(),
            pv_table: self.pv_table.clone(),
            pool: self.workers.pool.clone(),
            control: Control {
                reason: AtomicU8::new(0),
                stop: job.stop.clone(),
                deadline,
                node_limit: job.node_limit,
                issued: Padded(AtomicU64::new(0)),
            },
            generation: self.generation,
            stats: Padded(Mutex::new(SearchStats::default())),
            pending: Padded(AtomicUsize::new(0)),
            pending_lock: Mutex::new(()),
            pending_ready: Padded(std::sync::Condvar::new()),
            parallel: self.options.threads > 1,
            split_empties: tree::split_empties(self.options.threads),
            // 各イテレーションが開始前に自身の選択率ステージを設定します。
            quality: AtomicU8::new(0),
            rules: self.calibration.clone(),
            sigmas: self.sigmas,
            // 時間制限付きレベル（timed_levels）では、選択ステージをそれ自体が目標レベルとして扱い
            // 通常の ProbCut 範囲を使います。固定の完全読み探索では準備段階として扱います。
            cache_epoch: self.cache_epoch,
            endgame_probcut_min: if job.mode == SearchMode::Exact && !job.timed_levels {
                endgame::EXACT_PREPARATION_PROBCUT_MIN
            } else {
                endgame::PROBCUT_MIN_EMPTIES
            },
        })
    }
    /// 反復深化スケジュール（評価探索のイテレーション群、および終盤要求時は選択ステージ＋最終完全読み）を構築します。
    fn plan(&self, job: &Job) -> Vec<Iteration> {
        let board = job.board;
        if job.timed_levels {
            let max_level = job.level.expect("timed levels use a level goal");
            return timed_level_iterations(max_level, board.empty_count());
        }
        let target = match job.mode {
            SearchMode::Depth(d) => d.min(board.empty_count()),
            SearchMode::Exact | SearchMode::SelectiveEndgame => board.empty_count(),
        };
        let midgame = |depth, selectivity| Iteration {
            depth,
            exact: false,
            selectivity,
        };
        let endgame = |selectivity| Iteration {
            depth: board.empty_count(),
            exact: true,
            selectivity,
        };
        let exact = matches!(job.mode, SearchMode::Exact | SearchMode::SelectiveEndgame);
        let depths = if exact {
            // 参照ソルバーの終盤準備深さと 2 手刻みのステップに合わせます。
            let warmup = target
                .saturating_sub(8)
                .clamp(2, 24)
                .min(target)
                .min(job.level.unwrap_or(60));
            iterative_depths(warmup)
        } else {
            iterative_depths(target)
        };
        // 評価探索のイテレーションは指定された選択率を使います。
        // 終盤探索の直前に行う場合は置換表と着手順序の準備だけが目的のため、
        // 最も軽い選択率ステージ（83%）で実行し、その後の終盤探索で 85% から目標ステージまで段階的に上げます。
        let prepare = exact && !self.calibration.is_empty();
        let warmup = if prepare {
            Selectivity::Percent83
        } else {
            job.selectivity
        };
        let mut iterations: Vec<Iteration> = depths
            .into_iter()
            .map(|depth| midgame(depth, warmup))
            .collect();
        if prepare {
            let mut stage = job.selectivity.min(Selectivity::Percent85);
            while stage < job.selectivity {
                iterations.push(endgame(stage));
                stage = next_stage(stage, job.selectivity, target);
            }
        }
        if exact {
            iterations.push(endgame(job.selectivity));
        }
        iterations
    }
}

/// ルート局面に対して解決された実行ジョブです。
struct Job {
    minimum_score: Option<i32>,
    reuse_pv: bool,
    keep_generation: bool,
    recover_endgame_pv: bool,
    recover_pv: bool,
    skip_pv_recovery: bool,
    board: Board,
    mode: SearchMode,
    selectivity: Selectivity,
    /// `Goal::Level` または `Goal::Plan` で指定された中盤の探索深さです（終盤準備探索の深さ上限も兼ねます）。
    level: Option<u8>,
    time_limit: Option<Duration>,
    timed_levels: bool,
    node_limit: Option<u64>,
    all_moves: bool,
    move_pvs: bool,
    review_move: Option<Square>,
    root_moves: Option<u64>,
    preferred_move: Option<Square>,
    stop: StopHandle,
}

fn new_tables(tt_mib: usize) -> Result<(Arc<Table>, Arc<Table>), EngineError> {
    let table = Table::new(tt_mib).map_err(EngineError::Memory)?;
    let pv_table = Table::new((tt_mib / 16).max(1)).map_err(EngineError::Memory)?;
    Ok((Arc::new(table), Arc::new(pv_table)))
}

/// 最初の反復が完了する前の初期結果です（合法手があれば先頭の合法手を仮の最善手とします）。
fn initial_result(board: Board) -> SearchResult {
    let moves = board.get_moves();
    let best_move = if moves != 0 {
        Some(Action::Place(Square::lowest(moves)))
    } else if board.get_opponent_moves() != 0 {
        Some(Action::Pass)
    } else {
        None
    };
    SearchResult {
        best_move,
        score: None,
        warm_score: None,
        score_is_upper_bound: false,
        kind: None,
        pv: Vec::new(),
        moves: Vec::new(),
        move_pvs: Vec::new(),
        stop_reason: StopReason::Completed,
        stats: SearchStats::default(),
        elapsed: Duration::ZERO,
        root_moves: moves,
        root_restricted: false,
    }
}

/// イテレーションが実際に手を探索したかを返します（深さ 0 は静的評価だけを行います）。
fn searched(kind: ResultKind) -> bool {
    !matches!(kind, ResultKind::Depth { depth: 0, .. })
}

/// `Engine::search` におけるルート探索の 1 イテレーション分の設定です。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Iteration {
    depth: u8,
    /// 静的評価ではなく終局まで読み切るかどうかを表します。
    exact: bool,
    selectivity: Selectivity,
}
impl Iteration {
    fn spec(self) -> SearchSpec {
        SearchSpec {
            depth: self.depth,
            exact: self.exact,
            probcut: true,
        }
    }
    fn kind(self) -> ResultKind {
        let selectivity = self.selectivity;
        if self.exact {
            ResultKind::Endgame { selectivity }
        } else {
            ResultKind::Depth {
                depth: self.depth,
                selectivity,
            }
        }
    }
    /// 完了したイテレーションの結果 `line` を `SearchResult` に反映します。
    fn record(self, result: &mut SearchResult, line: &Line) {
        result.kind = Some(self.kind());
        result.score = Some(line.score);
        result.pv = line.moves();
        if let Some(action) = result.pv.first() {
            result.best_move = Some(*action)
        }
        // 深さ 0 は局面を静的評価するだけなので、探索した最善手順（PV）としては記録しません。
        if self.depth == 0 {
            result.pv.clear();
        }
    }
}

fn timed_level_iterations(max_level: u8, empties: u8) -> Vec<Iteration> {
    let mut steps: Vec<_> = (1..=max_level)
        .map(|level| {
            let (mode, selectivity) = level::plan(level).at(empties);
            let (depth, exact) = match mode {
                SearchMode::Depth(depth) => (depth, false),
                SearchMode::Exact | SearchMode::SelectiveEndgame => (empties, true),
            };
            Iteration {
                depth,
                exact,
                selectivity,
            }
        })
        .collect();
    steps.dedup();
    let endgame_start = steps.partition_point(|step| !step.exact);
    if let Some(&target) = steps[..endgame_start].last() {
        // テーブル切り替え時の探索深さとその偶奇（パリティ）をそのまま保ちます。
        steps.splice(
            ..endgame_start,
            iterative_depths(target.depth)
                .into_iter()
                .map(|depth| Iteration { depth, ..target }),
        );
    }
    steps
}

/// 目標深さの偶奇を保ったまま、可能であれば深さ 5 または 6 から 2 手刻みで反復深化の深さ列を生成します。
fn iterative_depths(target: u8) -> Vec<u8> {
    if target <= 1 {
        return vec![target];
    }
    let start = (6 - (target & 1)).min(target - 2);
    let start = if start == 0 { 2 - (target & 1) } else { start };
    (start..=target).step_by(2).collect()
}

fn split_calibration(c: Option<crate::model::Calibration>) -> (Vec<ProbCutRule>, ProbCutSigmas) {
    c.map_or((Vec::new(), ProbCutSigmas::NORMAL), |c| (c.rules, c.sigmas))
}

/// ProbCut ルール群と偏差倍率の整合性を検証します。
/// すべてのルールが盤面範囲内（`empties <= 60`, `shallow_depth < depth`）で、有限かつ正の傾き・偏差を持ち、
/// `(exact, depth, empties)` の重複がなく、各ステージ倍率が正・有限・単調非減少であることを確認します。
pub(crate) fn validate_calibration(
    rules: &[ProbCutRule],
    sigmas: &ProbCutSigmas,
) -> Result<(), EngineError> {
    if !sigmas.is_valid() {
        return Err(EngineError::InvalidCalibration(
            "ProbCut multiples must be positive, finite and non-decreasing",
        ));
    }
    let mut seen = vec![false; 2 * 61 * 61];
    for r in rules {
        if r.empties > 60
            || r.depth > r.empties
            || (r.exact && r.depth != r.empties)
            || r.depth < 2
            || r.shallow_depth >= r.depth
            || !r.slope.is_finite()
            || r.slope <= 0.0
            || !r.intercept.is_finite()
            || !r.deviation.is_finite()
            || r.deviation < 0.0
        {
            return Err(EngineError::InvalidCalibration(
                "invalid ProbCut regression",
            ));
        }
        let i = rule_index(r.exact, r.depth, r.empties);
        if std::mem::replace(&mut seen[i], true) {
            return Err(EngineError::InvalidCalibration(
                "duplicate ProbCut regression",
            ));
        }
    }
    Ok(())
}

fn rule_index(exact: bool, depth: u8, empties: u8) -> usize {
    (usize::from(exact) * 61 + usize::from(depth)) * 61 + usize::from(empties)
}

fn aspiration_window(pivot: i32, left: i32, right: i32, exact: bool) -> (i32, i32) {
    let mut alpha = pivot - left;
    let mut beta = pivot + right;
    if exact {
        alpha -= alpha.rem_euclid(2);
        beta += (-beta).rem_euclid(2);
    }
    (alpha.max(-65), beta.min(65))
}

fn indexed_rules(rules: Vec<ProbCutRule>) -> Arc<[Option<ProbCutRule>]> {
    let mut indexed = if rules.is_empty() {
        Vec::new()
    } else {
        vec![None; 2 * 61 * 61]
    };
    for rule in rules {
        indexed[rule_index(rule.exact, rule.depth, rule.empties)] = Some(rule);
    }
    indexed.into()
}

/// `current`（85% 以上）の次の終盤選択ステージを返します。
/// 空き 27 マス未満では即座に `target` へ飛び、30 マス未満では 95% の次に直接 `target` へ進みます。
fn next_stage(current: Selectivity, target: Selectivity, empties: u8) -> Selectivity {
    let next = current.next();
    if empties < 27 || (empties < 30 && next >= Selectivity::Percent98) {
        target
    } else {
        next.min(target)
    }
}

impl Engine {
    /// 評価重み（補助ニューラルネットを持つ場合はそれも含みます）を DPR2 形式で書き出します（較正データは含みません）。
    /// [`write_analysis_settings`](Self::write_analysis_settings) と組み合わせることで、
    /// Book ファイルなどに記録する解析値の互換性を識別できます。
    pub fn write_evaluation_model(&self, out: impl std::io::Write) -> std::io::Result<()> {
        self.model.write_to(out)
    }

    /// 現在の探索セマンティクス設定（互換性リビジョン、コンパイル時評価バリアント、
    /// 展開済み ProbCut 較正ルール・倍率）を安定した順序でシリアライズします。
    pub fn write_analysis_settings(&self, mut out: impl std::io::Write) -> std::io::Result<()> {
        out.write_all(b"deft-search-semantics")?;
        out.write_all(&SEARCH_SEMANTICS_REVISION.to_le_bytes())?;
        out.write_all(&[u8::from(cfg!(feature = "endgame-pattern-ordering"))])?;
        for rule in self.calibration.iter().flatten() {
            out.write_all(&[
                u8::from(rule.exact),
                rule.depth,
                rule.empties,
                rule.shallow_depth,
            ])?;
            for value in [rule.slope, rule.intercept, rule.deviation] {
                out.write_all(&value.to_bits().to_le_bytes())?;
            }
        }
        // 標準の倍率（ProbCutSigmas::NORMAL）のときは何も書かず、従来の設定バイト列と同じにします。
        if self.sigmas != ProbCutSigmas::NORMAL {
            out.write_all(b"sigmas")?;
            for value in self.sigmas.midgame.iter().chain(&self.sigmas.exact) {
                out.write_all(&value.to_bits().to_le_bytes())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn forks_share_only_immutable_weights_and_stable_resize_keeps_tables() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let fork = engine
            .fork(EngineOptions::default().threads(2).tt_mib(1))
            .unwrap();
        assert!(Arc::ptr_eq(&engine.model, &fork.model));
        assert!(Arc::ptr_eq(&engine.calibration, &fork.calibration));
        assert!(!Arc::ptr_eq(&engine.table, &fork.table));
        assert!(!Arc::ptr_eq(&engine.pv_table, &fork.pv_table));
        let table = engine.table.clone();
        engine.set_resources(engine.options()).unwrap();
        assert!(Arc::ptr_eq(&engine.table, &table));
    }
}

#[cfg(test)]
mod timed_level_tests {
    use super::*;

    #[test]
    fn depth_and_endgame_stages_follow_the_level_table() {
        use Selectivity::*;
        for (empties, level, first_depth, max_depth, endgame) in [
            (7, 60, 1, 0, &[Full][..]),
            (14, 1, 1, 1, &[]),
            (14, 60, 2, 4, &[Percent85, Full]),
            (24, 60, 6, 18, &[Percent85, Percent98, Percent99, Full]),
            (28, 21, 5, 21, &[]),
            (28, 22, 5, 21, &[Percent85]),
            (
                28,
                60,
                5,
                21,
                &[Percent85, Percent95, Percent98, Percent99, Full],
            ),
            (
                40,
                60,
                6,
                36,
                &[Percent85, Percent95, Percent98, Percent99, Full],
            ),
            (60, 60, 6, 56, &[Percent85, Percent98, Percent99, Full]),
        ] {
            let expected: Vec<_> = (first_depth..=max_depth)
                .step_by(2)
                .map(|depth| Iteration {
                    depth,
                    exact: false,
                    selectivity: Percent83,
                })
                .chain(endgame.iter().map(|&selectivity| Iteration {
                    depth: empties,
                    exact: true,
                    selectivity,
                }))
                .collect();
            assert_eq!(timed_level_iterations(level, empties), expected);
        }
    }
}

#[cfg(test)]
mod warm_start_tests {
    use super::*;

    #[test]
    fn warm_start_keeps_the_target_and_requires_the_cached_move_in_the_mask() {
        let mut engine = Engine::new(
            crate::Model::from_pattern(crate::PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let board = Board::new();
        let best = board.legal_moves().last().unwrap();
        engine.pv_table.store(
            board,
            crate::transposition::Entry {
                lower: 0,
                upper: 0,
                depth: 6,
                quality: Selectivity::Full.quality(),
                best: best.index(),
                second: 255,
                generation: 0,
                exact_mode: false,
            },
        );
        let request = SearchRequest::new(Goal::Depth(8)).reuse_pv();
        let stopped = engine
            .search(board, &request.clone().time_limit(Duration::ZERO))
            .unwrap();
        assert_eq!(stopped.best_move, Some(Action::Place(best)));
        assert_eq!(stopped.kind, None);
        assert_eq!(stopped.score, None);
        assert_eq!(stopped.warm_score, Some(0));

        let excluded = engine
            .search(
                board,
                &request.clone().root_moves(board.get_moves() & !best.bit()),
            )
            .unwrap();
        assert_eq!(excluded.stats.reused_iterations, 0);

        let warmed = engine.search(board, &request).unwrap();
        assert_eq!(warmed.stats.reused_iterations, 1);
        assert_eq!(
            warmed.kind,
            Some(ResultKind::Depth {
                depth: 8,
                selectivity: Selectivity::Full,
            })
        );
        assert_eq!(warmed.score, Some(0));
        assert!(!warmed.score_is_upper_bound);
    }

    #[test]
    fn keep_generation_preserves_tt_generation() {
        let mut engine = Engine::new(
            crate::Model::from_pattern(crate::PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let board = Board::new();
        engine
            .search(board, &SearchRequest::new(Goal::Depth(2)))
            .unwrap();
        let gen_after_first = engine.generation;
        engine
            .search(board, &SearchRequest::new(Goal::Depth(2)).keep_generation())
            .unwrap();
        assert_eq!(engine.generation, gen_after_first);
        engine
            .search(board, &SearchRequest::new(Goal::Depth(2)))
            .unwrap();
        assert_eq!(engine.generation, gen_after_first.wrapping_add(1));
    }
}
