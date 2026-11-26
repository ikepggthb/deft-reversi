//! 探索エンジンの公開 API で使うリクエスト・結果・設定・進捗・エラー型と ProbCut 設定型を定義します。
use super::*;

/// 永続化された推定探索値の互換性エポック番号です。
/// 評価関数・探索ロジック・MPC（Multi-ProbCut）の仕様（レベル表を含みます）が非互換に変更されたときに増やします。
/// スレッド数・制限時間・純粋な高速化だけの変更では増やす必要はありません。
pub const SEARCH_SEMANTICS_REVISION: u32 = 2;

/// 探索リクエストが解決される内部の探索モードです。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchMode {
    /// 指定深さ `Depth(d)` までの中盤探索です。
    Depth(u8),
    /// 完全読み（`Selectivity::Full`）です。事前に較正済みの選択探索で準備イテレーションを行う場合があります。
    Exact,
    /// 較正済みの確率枝刈り（ProbCut）を伴う終盤の読み切り探索です。
    SelectiveEndgame,
}
/// 探索が終了・停止した理由です。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    // 探索中は `AtomicU8` に格納されます。0 は「実行中（または正常完了）」を意味します。
    /// 要求された探索が最後まで完了しました。
    Completed = 0,
    /// [`StopHandle::stop`] またはハンドルのドロップによる外部からの停止要求で中断しました。
    Requested = 1,
    /// 指定された制限時間（`time_limit`）に達しました。
    TimeLimit = 2,
    /// 指定された訪問ノード数上限（`node_limit`）に達しました。
    NodeLimit = 3,
    /// ワーカースレッド内でパニックなどの致命的エラーが発生しました。
    WorkerFailed = 4,
}
/// 別スレッドから実行中の探索を停止するためのハンドルです。
/// クローン同士は同じ停止フラグを共有し、一度停止したハンドルは停止状態のままになるため、
/// 探索ごとに新しいリクエスト（ハンドル）を生成する必要があります。
#[derive(Clone, Default, Debug)]
pub struct StopHandle(pub(super) Arc<AtomicBool>);
impl StopHandle {
    /// 未停止状態の新しい [`StopHandle`] を生成します。
    pub fn new() -> Self {
        Self::default()
    }
    /// 探索へ停止を要求します。
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
    /// すでに停止要求が出されているかを返します。
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
/// エンジン起動時の構成オプションです。`EngineOptions::default().threads(n).tt_mib(m)` のように組み立てます。
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EngineOptions {
    /// 呼び出し元スレッドを含む探索スレッド数（1..=256）です。
    pub threads: usize,
    /// メイン置換表のサイズ（MiB）です。
    pub tt_mib: usize,
}
impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            threads: 1,
            tt_mib: 256,
        }
    }
}
impl EngineOptions {
    /// 探索スレッド数を設定します。
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }
    /// 置換表サイズ（MiB）を設定します。
    pub fn tt_mib(mut self, tt_mib: usize) -> Self {
        self.tt_mib = tt_mib;
        self
    }
}
/// ProbCut（浅い探索の結果から見込みの薄い枝を統計的に打ち切る手法）の信頼度ステージです。
/// 精度が低い順から、枝刈りをしない完全探索 `Full` までが並びます。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Selectivity {
    /// 選択率 83%。
    Percent83,
    /// 選択率 85%。
    Percent85,
    /// 選択率 95%。
    Percent95,
    /// 選択率 98%。
    Percent98,
    /// 選択率 99%。
    Percent99,
    /// 確率的枝刈りを行わない完全探索（100%）。
    #[default]
    Full,
}
impl Selectivity {
    /// 全ステージの一覧（精度が低い順）です。
    pub const ALL: [Self; 6] = [
        Self::Percent83,
        Self::Percent85,
        Self::Percent95,
        Self::Percent98,
        Self::Percent99,
        Self::Full,
    ];
    /// パーセント値（83, 85, 95, 98, 99, 100）を返します。
    pub fn percent(self) -> u8 {
        match self {
            Self::Percent83 => 83,
            Self::Percent85 => 85,
            Self::Percent95 => 95,
            Self::Percent98 => 98,
            Self::Percent99 => 99,
            Self::Full => 100,
        }
    }
    /// パーセント値（83, 85, 95, 98, 99, 100）に対応する [`Selectivity`] を返します。
    pub fn from_percent(percent: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.percent() == percent)
    }
    /// 1 段階精度が高いステージを返します（`Full` は `Full` のままです）。
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Percent83 => Self::Percent85,
            Self::Percent85 => Self::Percent95,
            Self::Percent95 => Self::Percent98,
            Self::Percent98 => Self::Percent99,
            Self::Percent99 | Self::Full => Self::Full,
        }
    }
    /// 置換表に格納する品質タグ値を返します（`Full` は 0、選択ステージは 1..=5 です）。
    pub(crate) fn quality(self) -> u8 {
        match self {
            Self::Full => 0,
            Self::Percent83 => 1,
            Self::Percent85 => 2,
            Self::Percent95 => 3,
            Self::Percent98 => 4,
            Self::Percent99 => 5,
        }
    }
}

/// 探索の目標（どこまで読むか）を表します。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// 指定手数（0..=60）まで先読みします（パスは手数に含めません）。
    Depth(u8),
    /// 終局まで読み切ります（`Selectivity::Full` 指定時は完全読みになります）。
    Endgame,
    /// 強さレベル（1..=60）です。レベル表に応じて中盤深さ・終盤切替空きマス数・選択率を決定します
    /// （`Plan(SearchPlan::level(n)?)` と等価です）。
    Level(u8),
    /// 中盤深さと終盤ステージごとの空きマス閾値を明示したカスタム [`SearchPlan`] です。
    Plan(SearchPlan),
}

/// 空きマス数に応じた探索計画です。
/// 空きマスが十分少なければ終盤ステージのいずれかで終局まで読み切り、そうでなければ中盤深さまで読みます。
///
/// ```
/// # use deft_reversi_engine::{SearchPlan, Selectivity};
/// // レベル 24 をベースに、完全読みの開始だけを空き 26 マスに変更します。
/// let plan = SearchPlan::level(24)?.endgame(Selectivity::Full, 26);
/// // 中盤は 83% で深さ 12、空き 20 マスから 95% 終盤読み、空き 16 マスから完全読みを行います。
/// let own = SearchPlan::midgame(12, Selectivity::Percent83)
///     .endgame(Selectivity::Percent95, 20)
///     .endgame(Selectivity::Full, 16);
/// # Ok::<(), deft_reversi_engine::EngineError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchPlan {
    midgame_depth: u8,
    midgame_selectivity: Selectivity,
    /// [`Selectivity`] を添字とし、そのステージで終局まで読み切る最大空きマス数を保持します（0 は未使用）。
    endgame: [u8; 6],
}
impl SearchPlan {
    /// レベル 1..=60 に対応する標準の [`SearchPlan`] を返します。
    pub fn level(level: u8) -> Result<Self, EngineError> {
        if !(1..=60).contains(&level) {
            return Err(EngineError::InvalidRequest("level must be 1..=60"));
        }
        Ok(level::plan(level))
    }
    /// すべての空きマス数で `depth`（0..=60）まで中盤探索する計画を生成します。
    pub fn midgame(depth: u8, selectivity: Selectivity) -> Self {
        Self {
            midgame_depth: depth,
            midgame_selectivity: selectivity,
            endgame: [0; 6],
        }
    }
    /// 終盤ステージの設定を保ったまま、中盤探索の深さと選択率を変更します。
    pub fn with_midgame(mut self, depth: u8, selectivity: Selectivity) -> Self {
        self.midgame_depth = depth;
        self.midgame_selectivity = selectivity;
        self
    }
    /// `up_to_empties` マス以下で選択率 `selectivity`（`Full` なら完全読み）の終盤探索を行うよう設定します。
    /// 0 を指定するとそのステージを無効化します。複数ステージが該当する場合は最も高精度なものが選ばれます。
    pub fn endgame(mut self, selectivity: Selectivity, up_to_empties: u8) -> Self {
        self.endgame[selectivity as usize] = up_to_empties;
        self
    }
    /// 中盤探索深さを返します。
    pub fn midgame_depth(&self) -> u8 {
        self.midgame_depth
    }
    /// 中盤探索の選択率を返します。
    pub fn midgame_selectivity(&self) -> Selectivity {
        self.midgame_selectivity
    }
    /// 指定選択率の終盤探索を適用する最大空きマス数を返します（未使用なら 0）。
    pub fn endgame_limit(&self, selectivity: Selectivity) -> u8 {
        self.endgame[selectivity as usize]
    }
    /// 深さと空きマス上限が 0..=60 の範囲内かを検証します。
    pub(crate) fn validate(&self) -> Result<(), EngineError> {
        if self.midgame_depth > 60 || self.endgame.iter().any(|&e| e > 60) {
            return Err(EngineError::InvalidRequest(
                "a plan's depth and empties must be 0..=60",
            ));
        }
        Ok(())
    }
    /// 空きマス数 `empties` の局面で適用される探索モードと選択率を返します。
    pub(crate) fn at(&self, empties: u8) -> (SearchMode, Selectivity) {
        let stage = Selectivity::ALL
            .into_iter()
            .rev()
            .find(|&stage| empties <= self.endgame[stage as usize]);
        match stage {
            Some(Selectivity::Full) => (SearchMode::Exact, Selectivity::Full),
            Some(stage) => (SearchMode::SelectiveEndgame, stage),
            None => (
                SearchMode::Depth(self.midgame_depth),
                self.midgame_selectivity,
            ),
        }
    }
}

/// 探索リクエストです。`SearchRequest::new(goal)` と各種ビルダーメソッドで組み立てます。
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub(super) goal: Goal,
    pub(super) selectivity: Option<Selectivity>,
    pub(super) time_limit: Option<Duration>,
    pub(super) timed_levels: bool,
    pub(super) node_limit: Option<u64>,
    pub(super) all_moves: bool,
    pub(super) move_pvs: bool,
    pub(super) review_move: Option<crate::Square>,
    pub(super) root_moves: Option<u64>,
    pub(super) preferred_move: Option<crate::Square>,
    pub(super) minimum_score: Option<i32>,
    pub(super) reuse_pv: bool,
    pub(super) recover_endgame_pv: bool,
    pub(super) recover_pv: bool,
    pub(super) skip_pv_recovery: bool,
    pub(super) stop: StopHandle,
}
impl SearchRequest {
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            selectivity: None,
            time_limit: None,
            timed_levels: false,
            node_limit: None,
            all_moves: false,
            move_pvs: false,
            review_move: None,
            root_moves: None,
            preferred_move: None,
            minimum_score: None,
            reuse_pv: false,
            recover_endgame_pv: false,
            recover_pv: false,
            skip_pv_recovery: false,
            stop: StopHandle::new(),
        }
    }
    /// 1 つの制限時間内で `level` まで反復深化探索を行い、最後に完了したイテレーションの最善手を返します。
    /// 較正済みモデルを使っている場合は、ルートの空きマス数とレベル表から最終探索深さと終盤ステージが決まります。
    /// 評価探索深さは 2 ずつ増加します。
    pub fn timed_levels(level: u8, limit: Duration) -> Self {
        Self {
            timed_levels: true,
            ..Self::new(Goal::Level(level)).time_limit(limit)
        }
    }
    /// `Depth` および `Endgame` 用の ProbCut 選択率を設定します（未指定時は `Full`）。
    /// `Level` は自身のスケジュールを持つため、`Level` に対して指定すると無効リクエストエラーになります。
    pub fn selectivity(mut self, selectivity: Selectivity) -> Self {
        self.selectivity = Some(selectivity);
        self
    }
    /// ルートで探索する合法手をビットマスクで限定します（子孫ノードは制限されません）。
    /// 部分集合の探索結果は選択された手の中での最善だけを保証し、局面全体の最善は保証しません。
    /// 強制パスや終局局面では 0 だけが有効なマスクになります。
    pub fn root_moves(mut self, mask: u64) -> Self {
        self.root_moves = Some(mask);
        self
    }
    /// 最初に探索する優先手のヒントを設定します（選択された `root_moves` 外なら無視されます）。
    pub fn preferred_move(mut self, square: crate::Square) -> Self {
        self.preferred_move = Some(square);
        self
    }
    /// 最終中盤深さにおいて、選択された手の中に `score` 以上に到達する手があるかを先に検証します。
    /// 失敗時はマスク全体の上界だけを返し、成功時は通常の点スコア探索へ進みます。
    pub fn minimum_score(mut self, score: i32) -> Self {
        self.minimum_score = Some(score);
        self
    }
    /// 互換性のある置換表エントリをウォームスタート（初期反復の省略）として再利用します。
    /// 目標イテレーション自体は要求されたルートマスクで引き続き探索します。
    pub fn reuse_pv(mut self) -> Self {
        self.reuse_pv = true;
        self
    }
    /// 完了した終盤選択ステージで短くなった PV（最善応手手順）の延長を試みます。
    /// 復元処理も探索の時間・ノード上限を共有するため、途中で打ち切られる場合があります。
    pub fn recover_endgame_pv(mut self) -> Self {
        self.recover_endgame_pv = true;
        self
    }
    /// 中盤・終盤を問わず、置換表カットなどで短くなった選択探索の PV を完了ステージで延長します。
    /// 最善手とスコアは変更されません。復元処理も探索上限を共有するため部分列になる場合があります。
    pub fn recover_pv(mut self) -> Self {
        self.recover_pv = true;
        self
    }
    /// 探索中に得られた PV をそのまま返し、個別の PV 復元パスを実行しません。
    /// 最善手とスコアは完了した探索からそのまま取得されます。
    pub fn skip_pv_recovery(mut self) -> Self {
        self.skip_pv_recovery = true;
        self
    }
    /// 指定時間経過後に停止を要求し、最後に完了したイテレーションの結果を返します。
    pub fn time_limit(mut self, limit: Duration) -> Self {
        self.time_limit = Some(limit);
        self
    }
    /// 全スレッド合計の訪問ノード数が上限に達したら停止します。
    pub fn node_limit(mut self, limit: u64) -> Self {
        self.node_limit = Some(limit);
        self
    }
    /// 選択されたすべてのルート合法手をフルウィンドウ（絞り込まない通常の探索窓）で再探索し、
    /// 各手のスコアを `SearchResult::moves` に格納します。各手を個別に探索するため数倍の時間がかかります。
    pub fn all_moves(mut self) -> Self {
        self.all_moves = true;
        self
    }
    /// 選択されたすべてのルート合法手をスコア付けし、各手の PV も保持します（`SearchResult::move_pvs`）。
    /// 置換表カットオフにより一部が短い PV になる場合があります。
    pub fn all_moves_with_pv(mut self) -> Self {
        self.all_moves = true;
        self.move_pvs = true;
        self
    }
    /// 最終探索ステージで最善手および指定した合法手 `square` をフルウィンドウで評価します（指し手レビュー用）。
    /// 完全な PV 復元はスキップされます。`all_moves` が設定されている場合は全ルート手を評価します。
    pub fn review_move(mut self, square: crate::Square) -> Self {
        self.review_move = Some(square);
        self
    }
    /// 内部で新規作成する代わりに外部の [`StopHandle`] を使います。
    /// 1 つのハンドルでバッチ処理中の複数探索を一括停止したい場合に用います。
    pub fn with_stop(mut self, stop: StopHandle) -> Self {
        self.stop = stop;
        self
    }
    /// このリクエストに紐づく [`StopHandle`] のクローンを返します。
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
    /// このリクエストの探索目標 [`Goal`] を返します。
    pub fn goal(&self) -> Goal {
        self.goal
    }
}
/// 1 回の探索の統計カウンタです。並列関連フィールド（`splits` 〜 `join_time`）は診断用でありバージョン間で変わり得ます。
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SearchStats {
    pub nodes: u64,
    pub leaves: u64,
    pub evaluations: u64,
    pub tt_hits: u64,
    pub tt_cuts: u64,
    pub beta_cuts: u64,
    pub splits: u64,
    pub helper_jobs: u64,
    pub ancestor_handoffs: u64,
    pub worker_dispatches: u64,
    /// 各マスターの合流待ち時間の合計です（待機中に子孫ジョブを手伝った時間も含みます）。
    pub join_time: Duration,
    pub probcut_tries: u64,
    pub probcut_cuts: u64,
    pub stability_cuts: u64,
    /// 互換性のある置換表エントリを利用してスキップした初期イテレーション数です。
    pub reused_iterations: u64,
}
impl SearchStats {
    /// 別コンテキスト（ワーカースレッドなど）の統計を合算します。
    pub(super) fn merge(&mut self, other: &Self) {
        self.nodes += other.nodes;
        self.leaves += other.leaves;
        self.evaluations += other.evaluations;
        self.tt_hits += other.tt_hits;
        self.tt_cuts += other.tt_cuts;
        self.beta_cuts += other.beta_cuts;
        self.splits += other.splits;
        self.helper_jobs += other.helper_jobs;
        self.ancestor_handoffs += other.ancestor_handoffs;
        self.worker_dispatches += other.worker_dispatches;
        self.join_time += other.join_time;
        self.probcut_tries += other.probcut_tries;
        self.probcut_cuts += other.probcut_cuts;
        self.stability_cuts += other.stability_cuts;
        self.reused_iterations += other.reused_iterations;
    }
}
/// 完了した 1 イテレーションの探索種別です。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    /// 指定深さ `depth`・選択率 `selectivity` の中盤探索です。
    Depth { depth: u8, selectivity: Selectivity },
    /// 選択率 `selectivity` の終盤読み切り探索です。
    Endgame { selectivity: Selectivity },
}
impl ResultKind {
    /// このイテレーションの選択率を返します。
    pub fn selectivity(self) -> Selectivity {
        match self {
            Self::Depth { selectivity, .. } | Self::Endgame { selectivity } => selectivity,
        }
    }
}

/// 1 つの着手とその評価スコア（石差）のペアです。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveScore {
    pub action: Action,
    pub score: i32,
}

/// 探索の最終結果です。スコアは常に手番側から見た石差を表します。
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SearchResult {
    /// 終局時だけ `None` になります。最初のイテレーション完了前に停止した場合でも合法手を 1 つ返します。
    /// マスク全体の上界だけ証明できた場合は、最適性を主張しない合法代表手を返します。
    pub best_move: Option<Action>,
    /// 1 つもイテレーションが完了しなかった場合は `None` になります。
    pub score: Option<i32>,
    /// 最終イテレーションがマスク全体に対する上界だけを証明した場合に `true` になります。
    pub score_is_upper_bound: bool,
    /// 最後に完了したイテレーション種別です（`score` が `None` なら `None`）。
    pub kind: Option<ResultKind>,
    /// 復元された最善応手手順（Principal Variation）です。
    pub pv: Vec<Action>,
    /// `all_moves` または `review_move` の要求時に、最終イテレーションで評価された各手のスコア（良い順）です。
    pub moves: Vec<MoveScore>,
    /// `all_moves_with_pv` の要求時に、`moves` の各手に対応して返される PV 列です。
    pub move_pvs: Vec<Vec<Action>>,
    /// 探索が停止した理由です。
    pub stop_reason: StopReason,
    /// 探索統計情報です。
    pub stats: SearchStats,
    /// 探索全体の経過時間です。
    pub elapsed: Duration,
    /// この結果の対象となったルート合法手マスクです（強制パス・終局時は 0）。
    pub root_moves: u64,
    /// 合法手のうち一部を意図的に除外して探索した場合に `true` になります。
    pub root_restricted: bool,
}
impl SearchResult {
    /// PV を終端まで再生したときの終局石差をルート手番視点で返します。
    /// PV が不完全または非合法手を含む場合は `None` を返します。
    pub fn pv_final_score(&self, position: impl Into<crate::Board>) -> Option<i32> {
        super::pv::terminal_score(position.into(), &self.pv)
    }
    /// `score` がゲーム全体の厳密な完全読み解（全合法手かつ `Selectivity::Full` で終盤完了）かを返します。
    pub fn is_solved(&self) -> bool {
        !self.root_restricted && self.is_subset_solved()
    }
    /// 選択されたルート手集合に対する厳密最大値が得られたかを返します。
    /// 除外された手については保証しないため、呼び出し側は結果を保存するときにマスクも記録してください。
    pub fn is_subset_solved(&self) -> bool {
        !self.score_is_upper_bound
            && self.kind
                == Some(ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                })
    }
}

/// 各イテレーション（または `all_moves` の各手）完了時に通知される進捗情報です。
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Progress {
    pub kind: ResultKind,
    pub score: i32,
    pub best_move: Option<Action>,
    pub pv: Vec<Action>,
    /// 現時点までの全スレッド合計訪問ノード数です。
    pub nodes: u64,
    pub elapsed: Duration,
    /// 次に実行予定のイテレーションです（最終イテレーション後は `None`）。
    /// 時間・ノード上限や停止要求により、次のイテレーション前に終了する場合があります。
    pub next: Option<ResultKind>,
    /// `all_moves` かつ最終イテレーション完了後：これまでスコアが確定した手（良い順）です。
    pub moves: Vec<MoveScore>,
}

/// エンジン構築・探索・モデル読み込みなどで発生するエラーです。
#[derive(Debug)]
#[non_exhaustive]
pub enum EngineError {
    InvalidOptions(&'static str),
    InvalidRequest(&'static str),
    InvalidCalibration(&'static str),
    Memory(&'static str),
    Thread(std::io::Error),
    Model(std::io::Error),
    /// 別の探索が実行中に [`EngineWorker::start`](crate::EngineWorker::start) を呼んだ場合です。
    Busy,
}
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(s)
            | Self::InvalidRequest(s)
            | Self::InvalidCalibration(s)
            | Self::Memory(s) => f.write_str(s),
            Self::Thread(e) => write!(f, "search worker could not start: {e}"),
            Self::Model(e) => write!(f, "engine model: {e}"),
            Self::Busy => f.write_str("a search is already running"),
        }
    }
}
impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Thread(e) | Self::Model(e) => Some(e),
            _ => None,
        }
    }
}

/// 各選択率ステージ（[`ProbCutSigmas::STAGES`]）において、中盤ルールおよび完全読みルールが
/// 枝刈り（カット）に用いる標準偏差倍率（σ 倍率）です。
///
/// 選択率 p のステージでは、標準化した予測誤差 `|deep - predicted| / deviation` のうち
/// 割合 `1 - p` が外側に落ちる位置でカットするのが理想です。
/// デフォルト（[`ProbCutSigmas::NORMAL`]）は正規分布を仮定しますが、実際の探索誤差は裾が重いため、
/// 通常は較正ファイルから実測の分位点を与えます。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbCutSigmas {
    pub midgame: [f64; 5],
    pub exact: [f64; 5],
}
impl ProbCutSigmas {
    /// 配列の各要素に対応する選択率ステージ（配列順）です。
    pub const STAGES: [Selectivity; 5] = [
        Selectivity::Percent83,
        Selectivity::Percent85,
        Selectivity::Percent95,
        Selectivity::Percent98,
        Selectivity::Percent99,
    ];
    /// 正規分布に基づく各ステージ確率の両側分位点です。
    pub const NORMAL: Self = Self {
        midgame: NORMAL_SIGMAS,
        exact: NORMAL_SIGMAS,
    };
    /// 置換表品質タグ `quality`（1..=5）と完全読みフラグに対応する偏差倍率を返します。
    pub(crate) fn get(&self, exact: bool, quality: u8) -> f64 {
        let stage = usize::from(quality) - 1;
        if exact {
            self.exact[stage]
        } else {
            self.midgame[stage]
        }
    }
    /// すべての倍率が有限の正値かつ単調非減少であるかを検証します。
    pub(crate) fn is_valid(&self) -> bool {
        [self.midgame, self.exact].iter().all(|s| {
            s.iter().all(|v| v.is_finite() && *v > 0.0) && s.windows(2).all(|w| w[0] <= w[1])
        })
    }
}
impl Default for ProbCutSigmas {
    fn default() -> Self {
        Self::NORMAL
    }
}
const NORMAL_SIGMAS: [f64; 5] = [1.37220, 1.43953, 1.960, 2.326, 2.576];

/// 局面条件（完全読み/中盤・深さ・空きマス数）ごとの ProbCut 線形回帰パラメータです。
#[derive(Clone, Copy, Debug)]
pub struct ProbCutRule {
    pub exact: bool,
    pub depth: u8,
    pub empties: u8,
    pub shallow_depth: u8,
    pub slope: f64,
    pub intercept: f64,
    pub deviation: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_are_ordered_and_tagged_consistently() {
        // `SearchPlan` はステージを添字として 1 つずつ上限値を保持します。
        for (index, stage) in Selectivity::ALL.into_iter().enumerate() {
            assert_eq!(stage as usize, index);
        }
        assert!(Selectivity::ALL.windows(2).all(|w| w[0] < w[1]));
        assert!(
            Selectivity::ALL
                .windows(2)
                .all(|w| w[0].percent() < w[1].percent())
        );
        assert_eq!(Selectivity::ALL[..5], ProbCutSigmas::STAGES);
        for stage in Selectivity::ALL {
            assert_eq!(Selectivity::from_percent(stage.percent()), Some(stage));
        }
        assert_eq!(Selectivity::from_percent(68), None);
        assert_eq!(Selectivity::from_percent(77), None);
        // テーブルの品質タグ: 完全探索は 0、それ以外は倍率スロット + 1 です。
        assert_eq!(Selectivity::Full.quality(), 0);
        for (slot, stage) in ProbCutSigmas::STAGES.into_iter().enumerate() {
            assert_eq!(usize::from(stage.quality()), slot + 1);
            let mut sigmas = ProbCutSigmas::NORMAL;
            sigmas.midgame[slot] = 0.5;
            sigmas.exact[slot] = 0.25;
            assert_eq!(sigmas.get(false, stage.quality()), 0.5);
            assert_eq!(sigmas.get(true, stage.quality()), 0.25);
        }
        assert!(ProbCutSigmas::NORMAL.is_valid());
        for pair in Selectivity::ALL.windows(2) {
            assert_eq!(pair[0].next(), pair[1]);
        }
        assert_eq!(Selectivity::Full.next(), Selectivity::Full);
    }
}
