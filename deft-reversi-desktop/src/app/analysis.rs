//! 解析モードの制御と探索結果キャッシュを管理するモジュール。
//!
//! 以下の 3 種類の解析を担当する:
//! - **表示中局面の段階的解析**: レベル 1, 4, 8, ... と 4 刻みで設定レベルまで徐々に読みを深くする
//! - **完全解析**: ユーザー操作に応じて、枝刈りなしの完全読み（選択率 100%）で終局まで読み切る
//! - **棋譜全体の一括解析**: 置換表（計算済み局面のキャッシュ）を効率よく再利用するため、終局側から初手へ向かって遡りながら各局面を評価する
//!
//! 表示中局面の解析では、全合法手をまとめて 1 回で探索するのではなく「1 手ずつ」個別に探索する。
//! こうすることで、最善手から順に結果が確定し次第すぐ画面へ表示でき、思考の途中経過もリアルタイムに確認できる。
use super::Effect;
pub use crate::domain::settings::AnalysisSettings;
use crate::{
    domain::{History, editor},
    services::engine::{Reading, Report, SearchId, SearchJob, SearchSpec},
};
use deft_reversi_engine::{Action, Color, Goal, Position, Selectivity};
use std::collections::HashMap;

/// 手番側から見た 1 手の評価値と、その読みの深さ情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveValue {
    /// 着手。
    pub action: Action,
    /// 評価値（石差）。
    pub score: i32,
    /// 読みの深さ・精度情報。
    pub reading: Option<Reading>,
    /// 現在探索中のため最新の完了イテレーションから得た暫定値であるか。
    pub provisional: bool,
}

/// 解析パネル／盤面に表示する 1 合法手の情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownMove {
    /// 着手。
    pub action: Action,
    /// 評価値（未探索なら `None`）。
    pub value: Option<MoveValue>,
    /// 現在探索中（または最後に完了した）レベルの値であれば `true`。
    /// 過去の浅いレベルの値は `false` となり UI 上で薄く表示される。
    pub fresh: bool,
}

/// 局面の全合法手がどこまで揃って読まれたかを表す深さ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MovesDepth {
    #[default]
    None,
    Level(u8),
    Exact,
}

/// 1 つの局面に対する解析結果キャッシュ。
#[derive(Clone, Debug)]
pub struct Analysis {
    /// 全合法手をカバーした探索から得られた局面評価値と読み筋（PV）。
    pub report: Option<Report>,
    /// 局面全体の評価値が得られた探索レベル（完全解析時や未完了時は `None`）。
    pub level: Option<u8>,
    /// 探索済みの各合法手の評価値（良い順）。
    pub moves: Vec<MoveValue>,
    /// 全合法手の探索進捗（一括解析では局面全体の値のみを得るため、候補手リストとは別に管理する）。
    moves_depth: MovesDepth,
    /// ユーザーにより途中停止された場合 `true`（再解析が指示されるまでこれ以上深追いしない）。
    settled: bool,
}

impl Analysis {
    /// 枝刈りなしの完全終局解析（選択率 100%）が完了しているかを返す。
    pub fn is_exact(&self) -> bool {
        self.report
            .as_ref()
            .is_some_and(|report| report.reading == Some(Reading::Endgame { percent: 100 }))
    }
}

/// 表示中局面の次の目標探索レベルを返す（1, 4, 8, 12, ... と 4 刻みで `target` まで上昇）。
pub fn next_level(searched: u8, target: u8) -> u8 {
    let next = match searched {
        0 => 1,
        1..=3 => 4,
        _ => (searched / 4 + 1) * 4,
    };
    next.min(target)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Shown,
    Exact,
    Batch,
}

struct Running {
    /// 現在実行中の探索 ID（1 手読み終えて次の手へ移る一瞬の間は `None` になる）。
    id: Option<SearchId>,
    job: Job,
    position: Position,
    /// 目標探索レベル（完全解析時はレベル上限がないため `None`）。
    level: Option<u8>,
    /// これから読む予定の残り合法手（`pop()` で末尾から取り出すため、優先したい手ほど後ろに積む）。
    pending: Vec<Action>,
    /// いま探索している 1 手。
    current: Option<Action>,
    /// このレベルでこれまでに読み終えた各手の評価値。
    values: Vec<MoveValue>,
    /// このレベルでこれまでに見つかった最善手の探索レポート（局面全体の評価値と読み筋に使う）。
    best: Option<Report>,
    /// いま探索している手の最新の途中経過レポート。
    live: Option<Report>,
    /// ユーザーが停止ボタンを押したか（`true` の場合、いま読んでいる手が終わり次第このレベルを確定させる）。
    stopping: bool,
}

/// 局面ごとの解析結果キャッシュと現在実行中の解析ジョブを管理する構造体。
#[derive(Default)]
pub struct Analyzer {
    /// 解析レベルおよび悪手判定閾値の設定。
    pub settings: AnalysisSettings,
    cache: HashMap<Position, Analysis>,
    running: Option<Running>,
    /// 一括解析で残っている局面キュー（末尾から取り出すため終局側が末尾）と総数。
    batch: Option<(Vec<Position>, usize)>,
}

/// 解析対象としてスコアを算出できる局面（終局・強制パスでなく、かつ最小石数以上）かを返す。
fn searchable(position: Position) -> bool {
    !position.is_terminal() && !position.must_pass() && editor::searchable(position)
}

/// 候補手リストを評価値の高い順（降順）に並べ替える。
fn sort_best_first(values: &mut [MoveValue]) {
    values.sort_by_key(|v| std::cmp::Reverse(v.score));
}

impl Analyzer {
    /// キャッシュ済みの局面解析結果を返す。
    pub fn get(&self, position: Position) -> Option<&Analysis> {
        self.cache.get(&position)
    }

    fn running_for(&self, position: Position) -> Option<&Running> {
        self.running
            .as_ref()
            .filter(|r| r.position == position && r.job != Job::Batch)
    }

    /// 指定局面を現在探索中であれば、最新の途中経過レポートを返す。
    pub fn live(&self, position: Position) -> Option<&Report> {
        self.running_for(position).and_then(|r| r.live.as_ref())
    }

    /// 指定局面で現在探索中の手と、その目標レベル（完全解析時は `None`）を返す。
    pub fn reading_move(&self, position: Position) -> Option<(Option<u8>, Action)> {
        self.running_for(position)
            .and_then(|r| Some((r.level, r.current?)))
    }

    /// 指定局面を現在探索中（一括解析を除く）であるかを返す。
    pub fn is_searching(&self, position: Position) -> bool {
        self.running_for(position).is_some()
    }

    /// 局面の評価値・最善手・読み筋を返す（完了済みレベルの結果、または現在レベルの暫定最善手）。
    pub fn report(&self, position: Position) -> Option<&Report> {
        self.cache
            .get(&position)
            .and_then(|a| a.report.as_ref())
            .or_else(|| self.running_for(position)?.best.as_ref())
    }

    /// 指定局面の全合法手を「現在レベルの最新値優先 → 評価値降順」で返す。
    pub fn moves(&self, position: Position) -> Vec<ShownMove> {
        let running = self.running_for(position);
        let older = self.cache.get(&position).map_or(&[][..], |a| &a.moves[..]);
        let mut shown: Vec<ShownMove> = position
            .legal_moves()
            .map(|square| {
                let action = Action::Place(square);
                let find =
                    |values: &[MoveValue]| values.iter().find(|v| v.action == action).cloned();
                match running.and_then(|r| find(&r.values)) {
                    Some(value) => ShownMove {
                        action,
                        value: Some(value),
                        fresh: true,
                    },
                    None => ShownMove {
                        action,
                        value: find(older),
                        fresh: running.is_none(),
                    },
                }
            })
            .collect();
        shown.sort_by_key(|m| {
            (
                !(m.fresh && m.value.is_some()),
                m.value.is_none(),
                std::cmp::Reverse(m.value.as_ref().map(|v| v.score)),
            )
        });
        shown
    }

    /// 指定局面で現在探索中のレベルを返す（完全解析時は `Some(None)`、テスト用）。
    #[cfg(test)]
    pub fn searching_level(&self, position: Position) -> Option<Option<u8>> {
        self.running_for(position).map(|r| r.level)
    }

    /// 表示中の局面で次に読むべき目標レベルを返す（すでに十分読まれていれば `None`）。
    fn wanted_level(&self, position: Position) -> Option<u8> {
        let target = self.settings.level;
        let Some(analysis) = self.cache.get(&position) else {
            return Some(next_level(0, target));
        };
        if analysis.settled {
            return None;
        }
        // 一括解析では局面全体の評価値だけを計算し、各候補手の評価値は調べない。
        // そのため局面自体のレベルに関わらず、候補手一覧の到達深さ（`moves_depth`）を基準に段階的に読み進める。
        let read = match analysis.moves_depth {
            MovesDepth::Exact => return None,
            MovesDepth::Level(level) => {
                if level >= target || analysis.is_exact() {
                    return None;
                }
                level
            }
            MovesDepth::None => 0,
        };
        Some(next_level(read, target))
    }

    /// 棋譜全体の一括解析進捗（`(完了局面数, 総局面数)`）を返す。
    pub fn batch_progress(&self) -> Option<(usize, usize)> {
        self.batch
            .as_ref()
            .map(|(queue, total)| (total - queue.len(), *total))
    }

    /// 棋譜の `index` 手目時点における解析結果を返す（強制パス局面の場合はパス後の局面を参照する）。
    pub fn analysis_at(&self, history: &History, index: usize) -> Option<&Analysis> {
        let mut position = history.position(index);
        if position.must_pass() && !position.is_terminal() {
            position = position.play(Action::Pass).ok()?;
        }
        self.cache.get(&position)
    }

    /// 棋譜の `index` 手目時点における黒番視点の評価値を返す。
    ///
    /// 終局局面では公式ルール準拠の確定石差を返し、強制パス局面では直後局面の評価値を引き継ぐ。
    pub fn black_value(&self, history: &History, index: usize) -> Option<i32> {
        let mut position = history.position(index);
        if position.is_terminal() {
            return Some(position.final_score(Color::Black));
        }
        if position.must_pass() {
            position = position.play(Action::Pass).ok()?;
        }
        let score = self.cache.get(&position)?.report.as_ref()?.score?;
        Some(match position.side_to_move() {
            Color::Black => score,
            Color::White => -score,
        })
    }

    /// 棋譜の `index` 手目の着手が最善手と比べて何石損したかを着手側視点で返す。
    pub fn loss(&self, history: &History, index: usize) -> Option<i32> {
        let Action::Place(_) = history.actions()[index] else {
            return None;
        };
        let mover = history.position(index).side_to_move();
        let before = self.black_value(history, index)?;
        let after = self.black_value(history, index + 1)?;
        Some(match mover {
            Color::Black => before - after,
            Color::White => after - before,
        })
    }

    /// 解析設定の変更を反映する。目標レベルが上がった場合は停止済みフラグを解除して再探索を許可する。
    pub fn settings_changed(&mut self, settings: AnalysisSettings) {
        if settings.level > self.settings.level {
            // 新しい目標レベルまで再び読み進められるようにする。
            for analysis in self.cache.values_mut() {
                analysis.settled = false;
            }
        }
        self.settings = settings;
    }

    /// 実行中の解析探索および一括解析キューをすべて破棄・キャンセルする。
    pub fn cancel(&mut self, effects: &mut Vec<Effect>) {
        self.batch = None;
        if let Some(running) = self.running.take()
            && running.id.is_some()
        {
            effects.push(Effect::CancelSearch);
        }
    }

    /// AI の対局探索へエンジンを譲るため、実行中の解析探索を一時中断する。
    ///
    /// 一括解析中の局面はキューへ戻し、AI の着手後に再開されるようにする。
    pub fn interrupt(&mut self, effects: &mut Vec<Effect>) {
        if let Some(running) = self.running.take() {
            if running.id.is_some() {
                effects.push(Effect::CancelSearch);
            }
            if running.job == Job::Batch
                && let Some((queue, _)) = &mut self.batch
            {
                queue.push(running.position);
            }
        }
    }

    /// 実行中の解析探索を途中結果を保持したまま停止し、一括解析も終了する。
    pub fn stop(&mut self, effects: &mut Vec<Effect>) {
        self.batch = None;
        let Some(running) = &mut self.running else {
            return;
        };
        running.stopping = true;
        if running.id.is_some() {
            effects.push(Effect::MoveNow);
            return;
        }
        // 手と手の切り替え中（探索未起動）であれば即座に完了処理を行う。
        let running = self.running.take().expect("checked above");
        self.finish_moves(running);
    }

    /// 指定局面のキャッシュを消去し、最低レベルから再解析できるようにする。
    pub fn reanalyze(&mut self, position: Position, effects: &mut Vec<Effect>) {
        if self.running_for(position).is_some() {
            return;
        }
        if self.running.as_ref().is_some_and(|r| r.job == Job::Batch) {
            self.cancel(effects);
        }
        self.cache.remove(&position);
    }

    /// 一括解析で `position` を再探索する必要がないか（完全解析済み、または `redo` 無効かつ設定レベル以上に達しているか）を返す。
    fn batch_done(&self, position: Position, redo: bool) -> bool {
        self.cache.get(&position).is_some_and(|a| {
            a.is_exact() || (!redo && a.level.is_some_and(|level| level >= self.settings.level))
        })
    }

    /// 棋譜全体の一括解析キューを構築する。
    pub fn start_batch(&mut self, history: &History, redo: bool) {
        let queue: Vec<Position> = (0..=history.actions().len())
            .map(|i| history.position(i))
            .filter(|&p| searchable(p) && !self.batch_done(p, redo))
            .collect();
        // 末尾から pop して終局側から遡って探索することで、置換表の結果を序盤側へ活かす。
        let total = queue.len();
        self.batch = (total > 0).then_some((queue, total));
    }

    /// 指定局面の完全解析（選択率 100% 終局読み）を 1 手ずつ開始する。
    pub fn exact(&mut self, position: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if !searchable(position) {
            return;
        }
        self.cancel(effects);
        self.begin_moves(Job::Exact, position, None, next_id, effects);
    }

    /// エンジンが空いているときに次の解析探索を起動する。
    ///
    /// 優先順位:
    /// 1. 実行中の表示局面レベルの次の手（または実行中の一括解析）
    /// 2. 一括解析キューの次の局面
    /// 3. 表示中局面の次のレベル
    ///
    /// 表示局面が切り替わっていた場合は古い表示局面の探索をキャンセルする。
    pub fn pump(&mut self, shown: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if let Some(running) = &self.running {
            if running.job == Job::Batch || running.position == shown {
                if running.id.is_none() {
                    self.next_move(next_id, effects);
                }
                return;
            }
            if running.id.is_some() {
                effects.push(Effect::CancelSearch);
            }
            self.running = None;
        }
        if let Some((queue, _)) = &mut self.batch {
            while let Some(position) = queue.pop() {
                // キューに積んだ後で、表示局面解析などにより完全解析が終わっていた場合はスキップする。
                if self.cache.get(&position).is_some_and(Analysis::is_exact) {
                    continue;
                }
                let level = self.settings.level;
                let spec = SearchSpec::new(Goal::Level(level));
                let id = Self::start(position, spec, next_id, effects);
                self.running = Some(Running {
                    id: Some(id),
                    job: Job::Batch,
                    position,
                    level: Some(level),
                    pending: Vec::new(),
                    current: None,
                    values: Vec::new(),
                    best: None,
                    live: None,
                    stopping: false,
                });
                return;
            }
            self.batch = None;
        }
        if !searchable(shown) {
            return;
        }
        if let Some(level) = self.wanted_level(shown) {
            self.begin_moves(Job::Shown, shown, Some(level), next_id, effects);
        }
    }

    /// `position` の全合法手を 1 手ずつ順番に読み始める。
    ///
    /// 前回のレベルで評価値が高かった手から先に読むことで、局面全体の評価値と最善手が早い段階で安定する。
    fn begin_moves(
        &mut self,
        job: Job,
        position: Position,
        level: Option<u8>,
        next_id: &mut u64,
        effects: &mut Vec<Effect>,
    ) {
        let legal = position.legal_moves();
        let older = self.cache.get(&position).map_or(&[][..], |a| &a.moves[..]);
        let mut order: Vec<Action> = older
            .iter()
            .map(|v| v.action)
            .filter(|a| matches!(a, Action::Place(s) if legal.contains(*s)))
            .collect();
        let rest: Vec<Action> = legal
            .map(Action::Place)
            .filter(|a| !order.contains(a))
            .collect();
        order.extend(rest);
        order.reverse();
        self.running = Some(Running {
            id: None,
            job,
            position,
            level,
            pending: order,
            current: None,
            values: Vec::new(),
            best: None,
            live: None,
            stopping: false,
        });
        self.next_move(next_id, effects);
    }

    /// 現在進行中レベルの次の 1 手の探索を開始する（残りがなければこのレベルを完了する）。
    fn next_move(&mut self, next_id: &mut u64, effects: &mut Vec<Effect>) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        let Some(action) = running.pending.pop() else {
            let running = self.running.take().expect("checked above");
            self.finish_moves(running);
            return;
        };
        let Action::Place(square) = action else {
            return;
        };
        let spec = match running.level {
            Some(level) => SearchSpec::new(Goal::Level(level)),
            None => SearchSpec {
                selectivity: Some(Selectivity::Full),
                ..SearchSpec::new(Goal::Endgame)
            },
        };
        let spec = SearchSpec {
            root_moves: Some(square.bit()),
            recover_pv: true,
            ..spec
        };
        running.id = Some(Self::start(running.position, spec, next_id, effects));
        running.current = Some(action);
        running.live = None;
    }

    fn start(
        position: Position,
        spec: SearchSpec,
        next_id: &mut u64,
        effects: &mut Vec<Effect>,
    ) -> SearchId {
        let id = SearchId(*next_id);
        *next_id += 1;
        effects.push(Effect::StartSearch {
            job: SearchJob { id, position, spec },
        });
        id
    }

    /// 1 手ずつの探索レベルを完了し、結果をキャッシュへ反映する。
    ///
    /// 全合法手を読み切った場合は前のレベルの結果を新しいレベルで置き換える。
    /// 途中で停止された場合は、今回読めた手だけを新しい結果に更新し、残りの手や局面全体の評価値は前回の結果を維持する。
    fn finish_moves(&mut self, running: Running) {
        let previous = self.cache.remove(&running.position);
        let legal = running.position.legal_moves().count();
        // 最後の手を停止で打ち切った場合、目標レベルに届かない浅い反復イテレーションの結果が
        // 混ざることがあるため、すべての手の読み精度が最善手と一致しているか確認する。
        let complete = running.values.len() == legal
            && running.best.as_ref().is_some_and(|best| {
                running
                    .values
                    .iter()
                    .all(|value| value.reading == best.reading)
                    && (running.job != Job::Exact
                        || best.reading == Some(Reading::Endgame { percent: 100 }))
            });
        let mut moves = running.values.clone();
        if let Some(previous) = &previous {
            moves.extend(
                previous
                    .moves
                    .iter()
                    .filter(|old| !running.values.iter().any(|v| v.action == old.action))
                    .cloned(),
            );
        }
        sort_best_first(&mut moves);
        let analysis = match (complete, previous) {
            (true, previous) => {
                // 一括解析などで今回のレベルより深い局面評価値がすでに得られている場合は、
                // 浅い値で上書きせず深い方の値を保持する。
                let deeper = previous.as_ref().filter(|p| {
                    running.job != Job::Exact
                        && (p.is_exact() || p.level.zip(running.level).is_some_and(|(p, r)| p > r))
                });
                let (report, level) = match deeper {
                    Some(p) => (p.report.clone(), p.level),
                    None => (running.best, running.level),
                };
                Analysis {
                    report,
                    level,
                    moves,
                    moves_depth: running.level.map_or(MovesDepth::Exact, MovesDepth::Level),
                    settled: running.stopping || running.job == Job::Exact,
                }
            }
            (false, Some(previous)) => Analysis {
                moves,
                settled: running.stopping || previous.settled,
                ..previous
            },
            (false, None) => {
                if moves.is_empty() {
                    return;
                }
                Analysis {
                    report: None,
                    level: None,
                    moves,
                    moves_depth: MovesDepth::None,
                    settled: true,
                }
            }
        };
        self.cache.insert(running.position, analysis);
    }

    /// 解析中の探索進捗イベントを反映し、現在探索中の手の暫定評価値を即時更新する。
    pub fn on_progress(&mut self, id: SearchId, report: &Report) -> bool {
        let Some(running) = self.running.as_mut().filter(|r| r.id == Some(id)) else {
            return false;
        };
        running.live = Some(report.clone());
        // 1 手の探索中も、完了した反復深化ごとの暫定値を即座に候補手リストへ反映する。
        if let (Some(action), Some(score)) = (running.current, report.score) {
            running.values.retain(|v| v.action != action);
            running.values.push(MoveValue {
                action,
                score,
                reading: report.reading,
                provisional: true,
            });
            sort_best_first(&mut running.values);
        }
        true
    }

    /// 解析探索の完了イベントを反映する。`id` がこの解析器の探索でない場合は `false` を返す。
    pub fn on_finished(&mut self, id: SearchId, report: Report) -> bool {
        let Some(running) = self.running.as_mut().filter(|r| r.id == Some(id)) else {
            return false;
        };
        if running.job == Job::Batch {
            let running = self.running.take().expect("checked above");
            self.finish_batch(running, report);
            return true;
        }
        running.id = None;
        running.live = None;
        if let Some(action) = running.current.take() {
            running.values.retain(|v| v.action != action);
            if let Some(score) = report.score {
                running.values.push(MoveValue {
                    action,
                    score,
                    reading: report.reading,
                    provisional: false,
                });
                sort_best_first(&mut running.values);
                if running
                    .best
                    .as_ref()
                    .is_none_or(|best| best.score.is_none_or(|b| score > b))
                {
                    running.best = Some(Report {
                        best_move: Some(action),
                        moves: Vec::new(),
                        ..report
                    });
                }
            }
        }
        if running.stopping || running.pending.is_empty() {
            let running = self.running.take().expect("checked above");
            self.finish_moves(running);
        }
        true
    }

    fn finish_batch(&mut self, running: Running, report: Report) {
        let previous = self.cache.remove(&running.position);
        let analysis = match previous {
            // 途中停止で評価値が得られなかった場合は、前回のキャッシュをそのまま残す。
            previous if report.score.is_none() => match previous {
                Some(previous) => previous,
                None => return,
            },
            Some(previous) if previous.is_exact() => previous,
            // 局面全体の評価値は新しい結果で更新しつつ、各候補手の個別評価値は
            // それぞれの読み深さ情報を保ったまま引き継ぐ。
            previous => Analysis {
                moves_depth: previous
                    .as_ref()
                    .map_or(MovesDepth::None, |p| p.moves_depth),
                moves: previous.map(|p| p.moves).unwrap_or_default(),
                report: Some(Report {
                    moves: Vec::new(),
                    ..report
                }),
                level: running.level,
                settled: false,
            },
        };
        self.cache.insert(running.position, analysis);
    }

    /// 解析探索の失敗イベントを処理し、該当する場合は一括解析キューも停止する。
    pub fn on_failed(&mut self, id: SearchId) -> bool {
        let failed = self.running.take_if(|r| r.id == Some(id)).is_some();
        if failed {
            self.batch = None;
        }
        failed
    }
}

#[cfg(test)]
fn empty_report() -> Report {
    Report {
        best_move: None,
        score: None,
        reading: None,
        pv: Vec::new(),
        moves: Vec::new(),
        nodes: 0,
        elapsed: std::time::Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Grouping;
    use deft_reversi_engine::Square;

    fn place(name: &str) -> Action {
        Action::Place(name.parse::<Square>().unwrap())
    }

    fn report(score: i32) -> Report {
        Report {
            score: Some(score),
            ..empty_report()
        }
    }

    fn read(score: i32, reading: Reading) -> Report {
        Report {
            reading: Some(reading),
            ..report(score)
        }
    }

    fn started(effects: &[Effect]) -> Vec<(SearchId, Position, SearchSpec)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::StartSearch { job, .. } => Some((job.id, job.position, job.spec)),
                _ => None,
            })
            .collect()
    }

    /// The move a search reads, from its root mask.
    fn move_of(spec: SearchSpec) -> Action {
        let mask = spec.root_moves.expect("one move at a time");
        assert_eq!(mask.count_ones(), 1);
        Action::Place(Square::new(mask.trailing_zeros() as u8).unwrap())
    }

    /// Reads every move of the level the analyzer starts, scoring each by
    /// `score`; returns the level.
    fn read_level(
        a: &mut Analyzer,
        shown: Position,
        ids: &mut u64,
        score: impl Fn(Action) -> i32,
    ) -> Option<Goal> {
        let mut goal = None;
        loop {
            let mut effects = Vec::new();
            a.pump(shown, ids, &mut effects);
            let Some(&(id, position, spec)) = started(&effects).first() else {
                return goal;
            };
            assert_eq!(position, shown);
            if goal.is_some_and(|g| g != spec.goal) {
                // The next level began: leave it for the caller.
                a.cancel(&mut Vec::new());
                return goal;
            }
            goal = Some(spec.goal);
            assert!(a.on_finished(id, report(score(move_of(spec)))));
        }
    }

    #[test]
    fn levels_rise_by_four_up_to_the_setting() {
        assert_eq!(next_level(0, 20), 1);
        assert_eq!(next_level(1, 20), 4);
        assert_eq!(next_level(4, 20), 8);
        assert_eq!(next_level(17, 20), 20);
        assert_eq!(next_level(0, 2), 1);
        assert_eq!(next_level(1, 2), 2);
    }

    #[test]
    fn the_shown_position_is_read_one_move_at_a_time_at_rising_levels() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let mut levels = Vec::new();
        let mut searches = 0;
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, position, spec)) = started(&effects).first() else {
                break;
            };
            assert_eq!(position, start);
            let Goal::Level(level) = spec.goal else {
                panic!("a level search");
            };
            if levels.last() != Some(&level) {
                levels.push(level);
            }
            let action = move_of(spec);
            // While it runs, nothing else starts.
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            assert!(effects.is_empty());
            assert_eq!(a.searching_level(start), Some(Some(level)));
            // f5 is the best move; each value shows as soon as it is read.
            let score = if action == place("f5") {
                i32::from(level)
            } else {
                -1
            };
            assert!(a.on_finished(id, report(score)));
            let shown = a.moves(start);
            assert!(shown.iter().any(|m| m.action == action
                && m.fresh
                && m.value.as_ref().unwrap().score == score));
            searches += 1;
        }
        assert_eq!(levels, [1, 4, 8, 12, 16, 20]);
        assert_eq!(searches, 6 * 4);
        let analysis = a.get(start).unwrap();
        let report = analysis.report.as_ref().unwrap();
        assert_eq!(report.score, Some(20));
        assert_eq!(report.best_move, Some(place("f5")));
        assert_eq!(analysis.moves.len(), 4);
        assert_eq!(analysis.moves[0].action, place("f5"));
    }

    #[test]
    fn a_move_shows_each_iteration_with_its_own_reading() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let mut effects = Vec::new();
        a.pump(start, &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        let action = move_of(spec);
        let depth = Reading::Depth {
            depth: 1,
            percent: 83,
        };
        a.on_progress(id, &read(3, depth));
        let value = a.moves(start).into_iter().find(|m| m.action == action);
        let value = value.unwrap().value.unwrap();
        assert!(value.provisional);
        assert_eq!((value.score, value.reading), (3, Some(depth)));
        // The moves not read yet have no value at the first level.
        assert!(a.moves(start).iter().filter(|m| m.value.is_none()).count() == 3);
    }

    #[test]
    fn a_stopped_level_keeps_what_it_read_each_with_its_reading() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let low = Reading::Depth {
            depth: 1,
            percent: 83,
        };
        let high = Reading::Depth {
            depth: 4,
            percent: 83,
        };
        let mut effects = Vec::new();
        // Level 1, every move.
        loop {
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            let (id, _, spec) = started(&effects)[0];
            if spec.goal != Goal::Level(1) {
                break;
            }
            a.on_finished(id, read(1, low));
        }
        // Level 4: one move read, then stopped while reading the next.
        let (id, _, spec) = started(&effects)[0];
        let first = move_of(spec);
        a.on_finished(id, read(9, high));
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        let (id, _, _) = started(&effects)[0];
        effects.clear();
        a.stop(&mut effects);
        assert_eq!(effects, [Effect::MoveNow]);
        a.on_finished(
            id,
            Report {
                score: None,
                ..empty_report()
            },
        );
        let analysis = a.get(start).unwrap();
        // The position keeps level 1's value; the move read at level 4 says so.
        assert_eq!(analysis.level, Some(1));
        assert_eq!(analysis.report.as_ref().unwrap().score, Some(1));
        assert_eq!(analysis.moves.len(), 4);
        let value = analysis.moves.iter().find(|v| v.action == first).unwrap();
        assert_eq!((value.score, value.reading), (9, Some(high)));
        assert!(
            analysis
                .moves
                .iter()
                .filter(|v| v.reading == Some(low))
                .count()
                == 3
        );
        // Stopped: nothing more until asked again.
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        assert!(started(&effects).is_empty());
        a.reanalyze(start, &mut effects);
        a.pump(start, &mut ids, &mut effects);
        assert_eq!(started(&effects)[0].2.goal, Goal::Level(1));
    }

    #[test]
    fn stopping_the_first_exact_search_keeps_moves_and_allows_a_record_search() {
        let history = History::new(Position::initial());
        let start = history.current();
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let exact = Reading::Endgame { percent: 100 };
        a.exact(start, &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        a.on_finished(id, read(-8, exact));
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        let (next, _, _) = started(&effects)[0];
        a.stop(&mut Vec::new());
        a.on_finished(next, empty_report());

        let analysis = a.get(start).unwrap();
        assert!(!analysis.is_exact());
        assert_eq!(analysis.moves.len(), 1);
        assert_eq!(analysis.moves[0].action, move_of(spec));
        assert_eq!(analysis.moves[0].reading, Some(exact));
        assert!(a.report(start).is_none());
        assert_eq!(a.black_value(&history, 0), None);

        effects.clear();
        a.start_batch(&history, true);
        a.pump(start, &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        assert_eq!(spec.root_moves, None);
        a.on_finished(id, report(6));
        assert_eq!(a.black_value(&history, 0), Some(6));
        assert_eq!(a.get(start).unwrap().moves.len(), 1);
    }

    #[test]
    fn an_earlier_iteration_on_the_last_move_does_not_complete_the_level() {
        let start = Position::initial();
        let mut a = Analyzer::default();
        let mut ids = 1;
        let exact = Reading::Endgame { percent: 100 };
        for index in 0..start.legal_moves().count() {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let (id, _, _) = started(&effects)[0];
            if index + 1 == start.legal_moves().count() {
                a.stop(&mut Vec::new());
                a.on_finished(id, read(-1, Reading::Endgame { percent: 95 }));
            } else {
                a.on_finished(id, read(2, exact));
            }
        }
        assert!(a.report(start).is_none());
        assert!(!a.get(start).unwrap().is_exact());
    }

    #[test]
    fn an_exact_record_value_still_gets_its_move_values() {
        let history = History::new(Position::initial());
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        let mut effects = Vec::new();
        a.pump(history.current(), &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        assert_eq!(spec.root_moves, None);
        a.on_finished(id, read(0, Reading::Endgame { percent: 100 }));
        effects.clear();
        a.pump(history.current(), &mut ids, &mut effects);
        let [(_, _, spec)] = started(&effects)[..] else {
            panic!("the moves are searched");
        };
        assert!(spec.root_moves.is_some());
        assert_eq!(spec.goal, Goal::Level(1));
    }

    #[test]
    fn after_the_record_search_the_moves_still_rise_level_by_level() {
        let history = History::new(Position::initial());
        let start = history.current();
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        let mut effects = Vec::new();
        a.pump(start, &mut ids, &mut effects);
        a.on_finished(started(&effects)[0].0, report(7));
        // Every move at each level before the next, as with no record search.
        let mut levels = Vec::new();
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, _, spec)) = started(&effects).first() else {
                break;
            };
            let Goal::Level(level) = spec.goal else {
                panic!("a level search");
            };
            if levels.last() != Some(&level) {
                levels.push(level);
            }
            a.on_finished(id, report(i32::from(level)));
            // The record search read deeper, so its value stands until the
            // moves reach its level.
            let analysis = a.get(start).unwrap();
            if level < 20 {
                assert_eq!(analysis.level, Some(20));
                assert_eq!(analysis.report.as_ref().unwrap().score, Some(7));
            }
        }
        assert_eq!(levels, [1, 4, 8, 12, 16, 20]);
        assert_eq!(a.report(start).unwrap().score, Some(20));
    }

    #[test]
    fn a_redo_searches_the_record_again_but_not_what_is_exact() {
        let mut history = History::new(Position::initial());
        history.play(place("f5"), Grouping::NewGroup).unwrap();
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        for finished in [report(1), read(0, Reading::Endgame { percent: 100 })] {
            let mut effects = Vec::new();
            a.pump(history.current(), &mut ids, &mut effects);
            a.on_finished(started(&effects)[0].0, finished);
        }
        a.start_batch(&history, false);
        assert_eq!(a.batch_progress(), None, "nothing new at this level");
        a.start_batch(&history, true);
        assert_eq!(a.batch_progress(), Some((0, 1)), "all but the exact one");
    }

    #[test]
    fn an_exact_level_ends_the_levels() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let exact = Reading::Endgame { percent: 100 };
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, _, _)) = started(&effects).first() else {
                break;
            };
            a.on_finished(id, read(2, exact));
        }
        let analysis = a.get(start).unwrap();
        assert!(analysis.is_exact());
        assert_eq!(analysis.level, Some(1));
    }

    #[test]
    fn moving_away_stops_the_old_search() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        let next = start.play(place("f5")).unwrap();
        effects.clear();
        a.pump(next, &mut ids, &mut effects);
        assert_eq!(effects[0], Effect::CancelSearch);
        assert_eq!(started(&effects)[0].1, next);
        // The old result arrives late and is not this search.
        assert!(!a.on_finished(SearchId(1), report(5)));
        assert!(a.get(start).is_none());
    }

    #[test]
    fn the_batch_runs_backwards_then_values_and_losses_follow() {
        let mut history = History::new(Position::initial());
        for m in ["f5", "d6", "c3"] {
            history.play(place(m), Grouping::NewGroup).unwrap();
        }
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        assert_eq!(a.batch_progress(), Some((0, 4)));
        // Scores for the side to move: 0, -1, +3 (a bad d6), -2.
        let scores = [0, -1, 3, -2];
        for expected in (0..4).rev() {
            let mut effects = Vec::new();
            a.pump(history.current(), &mut ids, &mut effects);
            let [(id, position, spec)] = started(&effects)[..] else {
                panic!("one search");
            };
            assert_eq!(position, history.position(expected));
            assert_eq!(spec.root_moves, None);
            a.on_finished(id, report(scores[expected]));
        }
        let mut effects = Vec::new();
        a.pump(history.current(), &mut ids, &mut effects);
        assert_eq!(a.batch_progress(), None);
        // The shown position still gets its moves read.
        assert!(started(&effects)[0].2.root_moves.is_some());

        assert_eq!(a.black_value(&history, 0), Some(0));
        assert_eq!(a.black_value(&history, 1), Some(1));
        assert_eq!(a.black_value(&history, 2), Some(3));
        // f5 gained a disc; d6 took White from -1 to -3.
        assert_eq!(a.loss(&history, 0), Some(-1));
        assert_eq!(a.loss(&history, 1), Some(2));
    }

    #[test]
    fn the_exact_solve_reads_the_moves_best_first() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let goal = read_level(&mut a, start, &mut ids, |action| {
            if action == place("d3") { 2 } else { 0 }
        });
        assert_eq!(goal, Some(Goal::Level(1)));
        let mut effects = Vec::new();
        a.exact(start, &mut ids, &mut effects);
        let (_, _, spec) = *started(&effects).last().unwrap();
        assert_eq!(spec.goal, Goal::Endgame);
        assert_eq!(spec.selectivity, Some(Selectivity::Full));
        // The last level's best move comes first.
        assert_eq!(move_of(spec), place("d3"));
        let exact = Reading::Endgame { percent: 100 };
        let mut id = started(&effects).last().unwrap().0;
        loop {
            a.on_finished(id, read(0, exact));
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            match started(&effects).first() {
                Some(&(next, _, _)) => id = next,
                None => break,
            }
        }
        let analysis = a.get(start).unwrap();
        assert!(analysis.is_exact());
        assert_eq!(analysis.level, None);
        assert!(analysis.moves.iter().all(|v| v.reading == Some(exact)));
    }
}
