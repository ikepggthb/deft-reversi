//! book の育成。
//!
//! # 何を変えたか
//!
//! Edax も Egaroucid も、育成の基本は「book の端 (leaf) を全部そのまま
//! 1 段ずつ広げる」という幅優先で、どこを先に掘るかという順序を持たない。
//! Edax の `book_deviate` には「最善からどれだけ離れた変化まで追うか」の
//! 制限があるが、その範囲の中では順序を付けない。
//!
//! ここは **最良優先 (best-first)** にした。掘る候補に優先度を付け、
//! 良いものから順に予算 (局面数・時間) の限り掘る。優先度は
//!
//! 1. **根からの損** — 自分の手で許す損と相手の手で許す損を別々に数える。
//!    自分は最善しか選ばないが相手は間違えるかもしれない、という非対称性が
//!    実戦の book では本質的なので、1 本の数字にまとめない
//! 2. **値の不確かさ** ([`BookValue::uncertainty`]) — 分かっていないところを
//!    先に掘る。上下界が閉じている局面をさらに掘っても book は強くならない
//! 3. **浅さ** — 同じなら根に近い方を先に。序盤ほど使われる回数が多い
//!
//! の順に見る。2 が効くのは、この book が上下界を子から積み上げているから
//! ([`Book::propagate`])。「登録済みの手の中の最善」が下界、「まだ登録して
//! いない手を含めた見積もり」が上界なので、幅が広い局面はそのまま
//! 「まだ調べ足りない局面」になっている。
//!
//! # 1 局面あたりの探索
//!
//! 未登録の手 `M` を持つ局面 `P` について 2 回探索する。
//!
//! 1. `solve_with_moves(P, level, M)` — `M` の中の最善手 `m` と、その値。
//!    `P → m` の子局面を book に足す
//! 2. `solve_with_moves(P, level, M \ {m})` — 残りの中の最善手。
//!    これが新しい [`Frontier`] になり、`P` の値の**上界**を決める
//!
//! 2 回目は 1 回目で暖まった置換表に乗るので実際には安い。Edax も
//! Egaroucid も 1 回しか探索しないので上界が分からない。

use super::layer::ply_of;
use super::value::{clamp_score, BookValue, Frontier};
use super::{Book, PositionId};
use crate::board::board::Board;
use crate::search::{solver_type_for_level, Solver, SolverType};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 育成の方針。
#[derive(Clone, Debug)]
pub struct GrowthPolicy {
    /// 局面を評価する探索レベル。
    pub level: i32,
    /// これより深い局面は育てない。
    pub max_ply: usize,
    /// 自分の手で許す損の累計 (石差)。
    pub player_error: i32,
    /// 相手の手で許す損の累計 (石差)。
    pub opponent_error: i32,
    /// 追加する局面数の上限。
    pub max_positions: usize,
    /// 打ち切る時間。
    pub time_limit: Option<Duration>,
    /// 同時に探索する局面数。
    pub threads: NonZeroUsize,
    /// 外から止めるためのフラグ。
    pub stop: Option<Arc<AtomicBool>>,
}

impl Default for GrowthPolicy {
    fn default() -> Self {
        Self {
            level: 21,
            max_ply: 30,
            player_error: 0,
            opponent_error: 4,
            max_positions: 100,
            time_limit: None,
            threads: NonZeroUsize::MIN,
            stop: None,
        }
    }
}

/// 育成が止まった理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// 方針の範囲に掘るところがもう無い。
    Exhausted,
    /// 局面数の上限に達した。
    PositionBudget,
    /// 時間切れ。
    TimeLimit,
    /// 外から止められた。
    Interrupted,
}

/// 育成の結果。
#[derive(Clone, Debug)]
pub struct GrowthReport {
    /// 追加した局面数。
    pub added: usize,
    /// 探索した局面数 (候補の数)。
    pub searched: usize,
    /// 繰り返した回数。
    pub rounds: usize,
    /// かかった時間。
    pub elapsed: Duration,
    /// 止まった理由。
    pub stopped_by: StopReason,
}

/// 掘る候補 1 つ。
#[derive(Clone, Copy, Debug)]
struct Candidate {
    id: PositionId,
    board: Board,
    /// まだ book に無い合法手。
    unregistered: u64,
    /// 根からの損の合計。
    error: i32,
    /// 値の不確かさ。
    uncertainty: i32,
}

impl Candidate {
    /// 小さいほど先に掘る。
    fn priority(&self) -> (i32, std::cmp::Reverse<i32>, u8) {
        (self.error, std::cmp::Reverse(self.uncertainty), self.id.ply)
    }
}

/// 根からの到達のしかた。
#[derive(Clone, Copy, Debug)]
struct Reach {
    player_error: i32,
    opponent_error: i32,
    /// 0 なら根と同じ手番。
    side: u8,
}

impl Reach {
    fn total(&self) -> i32 {
        self.player_error + self.opponent_error
    }
}

/// 1 局面分の探索結果。
struct Expansion {
    id: PositionId,
    /// 追加する子局面と、その手番から見た値。
    child: Option<(Board, BookValue)>,
    /// 新しい frontier (正規形ではなく親の向きの座標)。
    frontier: Frontier,
}

/// book を育てる。
///
/// 呼ぶ前に [`Book::propagate`] を呼ぶ必要はない (中で呼ぶ)。
pub fn grow(book: &mut Book, solver: &Solver, policy: &GrowthPolicy) -> GrowthReport {
    let start = Instant::now();
    let mut report = GrowthReport {
        added: 0,
        searched: 0,
        rounds: 0,
        elapsed: Duration::ZERO,
        stopped_by: StopReason::Exhausted,
    };

    loop {
        if is_interrupted(policy) {
            report.stopped_by = StopReason::Interrupted;
            break;
        }
        if let Some(limit) = policy.time_limit {
            if start.elapsed() >= limit {
                report.stopped_by = StopReason::TimeLimit;
                break;
            }
        }
        if report.added >= policy.max_positions {
            report.stopped_by = StopReason::PositionBudget;
            break;
        }

        book.propagate();
        let mut candidates = collect_candidates(book, policy);
        if candidates.is_empty() {
            report.stopped_by = StopReason::Exhausted;
            break;
        }
        candidates.sort_by_key(Candidate::priority);

        // 1 巡で扱う数。残りの予算を超えない範囲で全候補を取る。
        let budget = policy.max_positions - report.added;
        let batch = candidates.len().min(budget).max(1);
        candidates.truncate(batch);

        let expansions = search_candidates(solver, policy, &candidates);
        report.searched += candidates.len();
        report.rounds += 1;

        let mut added_here = 0;
        for expansion in expansions {
            let board = *book.table().board(expansion.id);
            if let Some((child, value)) = expansion.child {
                if !book.contains(&child) {
                    added_here += 1;
                }
                book.insert(&child, value);
            }
            // 子を足したあとで frontier を決める。複数の手が同じ正規形に
            // 落ちることがあり (初期盤面の 4 手がその例)、1 つ足しただけで
            // 残りの手まで登録済みになることがある。
            let unregistered = book.unregistered_moves(&board);
            let frontier = if unregistered == 0 {
                Frontier::none()
            } else if expansion.frontier.has_move()
                && unregistered & (1u64 << expansion.frontier.mv) == 0
            {
                Frontier::unset()
            } else {
                expansion.frontier
            };
            book.set_frontier(&board, frontier);
        }
        report.added += added_here;

        if added_here == 0 {
            // 掘れる手が無くなった (frontier が exhausted になっただけ)。
            // 候補が尽きたわけではないので、次の巡で判定させる。
            book.propagate();
            if collect_candidates(book, policy).is_empty() {
                report.stopped_by = StopReason::Exhausted;
                break;
            }
        }
    }

    book.propagate();
    book.meta_mut().level = policy.level.clamp(0, 255) as u8;
    book.meta_mut().max_ply = policy.max_ply.min(255) as u8;
    book.meta_mut().player_error = policy.player_error.clamp(0, 255) as u8;
    book.meta_mut().opponent_error = policy.opponent_error.clamp(0, 255) as u8;
    report.elapsed = start.elapsed();
    report
}

fn is_interrupted(policy: &GrowthPolicy) -> bool {
    policy
        .stop
        .as_ref()
        .is_some_and(|s| s.load(Ordering::Relaxed))
}

/// `front` に `new` を追加する。`new` を支配する点があれば捨て、
/// `new` に支配される既存の点は取り除く。
fn add_pareto(front: &mut Vec<Reach>, new: Reach) {
    for existing in front.iter() {
        if existing.player_error <= new.player_error
            && existing.opponent_error <= new.opponent_error
        {
            return;
        }
    }
    front.retain(|r| r.player_error < new.player_error || r.opponent_error < new.opponent_error);
    front.push(new);
}

/// 根から損の範囲内で辿れる局面のうち、まだ掘れる手があるものを集める。
///
/// 子は必ず 1 つ深い層にいるので、浅い層から 1 回舐めるだけで到達可能性が
/// 決まる。キューも訪問済みの集合も要らない。
///
/// 到達経路は player_error と opponent_error の 2 次元で管理する。同じ局面に
/// 複数の経路で届いたとき、合計で比べると片方の予算が残っている経路を捨てて
/// しまう。パレート最適な経路だけを残すことで、予算が偏った変化も見逃さない。
fn collect_candidates(book: &Book, policy: &GrowthPolicy) -> Vec<Candidate> {
    let Some(max_ply) = book.max_ply() else {
        return Vec::new();
    };
    let limit = max_ply.min(policy.max_ply);

    let mut reach: Vec<Vec<Vec<Reach>>> = (0..=max_ply)
        .map(|ply| vec![Vec::new(); book.table().layer(ply).len()])
        .collect();

    let Some(root) = book.table().locate(&Board::new()) else {
        return Vec::new();
    };
    reach[root.ply as usize][root.slot as usize].push(Reach {
        player_error: 0,
        opponent_error: 0,
        side: 0,
    });

    let mut out = Vec::new();
    for ply in 0..=limit {
        for slot in 0..book.table().layer(ply).len() {
            if reach[ply][slot].is_empty() {
                continue;
            }
            let id = PositionId {
                ply: ply as u8,
                slot: slot as u32,
            };
            let board = *book.table().board(id);
            let children = book.children(&board);

            let registered: u64 = children.iter().map(|c| 1u64 << c.mv).fold(0, |a, b| a | b);
            let unregistered = board.moves() & !registered;
            if unregistered != 0 && ply < policy.max_ply {
                let error = reach[ply][slot]
                    .iter()
                    .map(|r| r.total())
                    .min()
                    .unwrap_or(0);
                out.push(Candidate {
                    id,
                    board,
                    unregistered,
                    error,
                    uncertainty: book.table().value(id).uncertainty(),
                });
            }

            if ply >= limit {
                continue;
            }

            let Some(best) = children
                .iter()
                .filter(|c| c.value.is_defined())
                .map(|c| c.value.score as i32)
                .max()
            else {
                continue;
            };

            let (before, after) = reach.split_at_mut(ply + 1);
            let here_front = &before[ply][slot];

            for child in &children {
                if !child.value.is_defined() {
                    continue;
                }
                let loss = best - child.value.score as i32;
                for here in here_front.iter() {
                    let next = if here.side == 0 {
                        Reach {
                            player_error: here.player_error + loss,
                            opponent_error: here.opponent_error,
                            side: if child.flips_turn { 1 } else { 0 },
                        }
                    } else {
                        Reach {
                            player_error: here.player_error,
                            opponent_error: here.opponent_error + loss,
                            side: if child.flips_turn { 0 } else { 1 },
                        }
                    };
                    if next.player_error > policy.player_error
                        || next.opponent_error > policy.opponent_error
                    {
                        continue;
                    }
                    let child_layer_idx = child.id.ply as usize - (ply + 1);
                    add_pareto(&mut after[child_layer_idx][child.id.slot as usize], next);
                }
            }
        }
    }
    out
}

/// 候補を並列に探索する。
///
/// 局面をばらまく方が探索の中を並列化するより効率が良いので、
/// スレッドは候補に割り当てる (`Solver` 自体は 1 スレッド設定で渡す)。
fn search_candidates(
    solver: &Solver,
    policy: &GrowthPolicy,
    candidates: &[Candidate],
) -> Vec<Expansion> {
    let n_threads = policy.threads.get().min(candidates.len().max(1));
    if n_threads <= 1 {
        return candidates
            .iter()
            .take_while(|_| !is_interrupted(policy))
            .map(|c| expand_one(solver, policy, c))
            .collect();
    }

    // 局面ごとに仕事の重さが大きく違うので、先着順で取らせる。
    // 結果の順序は使わない (書き戻しは [`PositionId`] で行う) ので、
    // 集約は 1 つの Mutex で足りる。1 局面の探索に比べれば無視できる。
    let next = AtomicUsize::new(0);
    let out = std::sync::Mutex::new(Vec::with_capacity(candidates.len()));

    std::thread::scope(|scope| {
        for _ in 0..n_threads {
            let next = &next;
            let out = &out;
            scope.spawn(move || {
                let mut local = Vec::new();
                loop {
                    if is_interrupted(policy) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= candidates.len() {
                        break;
                    }
                    local.push(expand_one(solver, policy, &candidates[i]));
                }
                out.lock().unwrap().extend(local);
            });
        }
    });

    out.into_inner().unwrap()
}

/// 候補 1 つを掘る。
fn expand_one(solver: &Solver, policy: &GrowthPolicy, candidate: &Candidate) -> Expansion {
    let board = candidate.board;
    let n_empties = board.empties_count() as u8;

    // 1 回目: 未登録の手の中の最善。
    let first = solver.solve_with_moves(&board, policy.level, candidate.unregistered);
    let Some(mv) = first.best_move else {
        return Expansion {
            id: candidate.id,
            child: None,
            frontier: Frontier::none(),
        };
    };
    let (depth, selectivity) = search_shape(&first.solver_type, n_empties);
    let child = board.make_move(1u64 << mv);
    // 子から見た値なので符号を反転する。子がパスするなら反転しない。
    let (child_board, sign) = if child.moves() == 0 && child.opponent_moves() != 0 {
        (child.passed(), 1)
    } else {
        (child, -1)
    };
    let child_value = BookValue::searched(
        clamp_score(sign * first.score),
        child_board.empties_count() as u8,
        depth.saturating_sub(1),
        selectivity,
    );

    // 2 回目: 残りの中の最善。これが新しい frontier になり、上界を決める。
    let rest = candidate.unregistered & !(1u64 << mv);
    let frontier = if rest == 0 {
        Frontier::none()
    } else {
        let second = solver.solve_with_moves(&board, policy.level, rest);
        match second.best_move {
            Some(next) => {
                let (depth, _) = search_shape(&second.solver_type, n_empties);
                Frontier {
                    mv: next as i8,
                    score: clamp_score(second.score),
                    depth,
                }
            }
            None => Frontier::none(),
        }
    };

    Expansion {
        id: candidate.id,
        child: Some((child_board, child_value)),
        frontier,
    }
}

/// 探索の構成から (深さ, 選択度) を取り出す。
fn search_shape(solver_type: &SolverType, n_empties: u8) -> (u8, u8) {
    match *solver_type {
        SolverType::Eval(depth, selectivity) => {
            (depth.clamp(0, 60) as u8, selectivity.clamp(0, 6) as u8)
        }
        SolverType::Final(selectivity) => (n_empties, selectivity.clamp(0, 6) as u8),
    }
}

/// レベルから、その局面で使われる探索の構成を先に知る。
///
/// 予定表示や、探索前に「この局面は完全読みになるか」を知りたいときに使う。
pub fn planned_search(board: &Board, level: i32) -> SolverType {
    solver_type_for_level(board.empties_count() as i32, level)
}

/// 棋譜の手順を book に足す。
///
/// 手順に現れる局面をそのまま登録するので、実戦で出る形を優先して
/// 育てたいときの下地になる。値は入れないので、あとで [`grow`] や
/// [`Book::propagate`] に任せる。追加した局面数を返す。
pub fn add_line(book: &mut Book, record: &[u8], max_ply: usize) -> usize {
    let mut board = Board::new();
    let mut added = 0;
    for &mv in record.iter().take(max_ply) {
        if board.moves() == 0 {
            if board.opponent_moves() == 0 {
                break;
            }
            board = board.passed();
        }
        if board.moves() & (1u64 << mv) == 0 {
            break;
        }
        board = board.make_move(1u64 << mv);
        if ply_of(&board) > max_ply {
            break;
        }
        if book.insert(&board, BookValue::undefined()) {
            added += 1;
        }
    }
    added
}

/// [`Frontier`] が指す手を、あらためて数え直す。
///
/// frontier が「もう掘れる手が無い」と言っているのに未登録の手が残っている、
/// といった食い違いを直す。直した局面数を返す。
pub fn fix_frontiers(book: &mut Book) -> usize {
    let Some(max_ply) = book.max_ply() else {
        return 0;
    };
    let mut fixed = 0;
    for ply in 0..=max_ply {
        for slot in 0..book.table().layer(ply).len() as u32 {
            let id = PositionId {
                ply: ply as u8,
                slot,
            };
            let board = *book.table().board(id);
            let unregistered = book.unregistered_moves(&board);
            let frontier = *book.table().frontier(id);

            let broken = if unregistered == 0 {
                !frontier.is_exhausted()
            } else {
                frontier.has_move() && unregistered & (1u64 << frontier.mv) == 0
            };
            if !broken {
                continue;
            }
            let replacement = if unregistered == 0 {
                Frontier::none()
            } else {
                Frontier::unset()
            };
            *book.table_mut().frontier_mut(id) = replacement;
            fixed += 1;
        }
    }
    fixed
}

// 候補をスレッドにばらまくので、両方を共有できる必要がある。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Solver>();
    assert_send_sync::<Book>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::value::{ValueFlags, SCORE_MAX};

    const D3: u8 = 19;

    fn policy() -> GrowthPolicy {
        GrowthPolicy {
            level: 1,
            max_ply: 4,
            player_error: 0,
            opponent_error: 0,
            max_positions: 4,
            ..GrowthPolicy::default()
        }
    }

    #[test]
    fn candidates_start_at_the_root() {
        let book = Book::new();
        let candidates = collect_candidates(&book, &policy());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id.ply, 0);
        assert_eq!(candidates[0].unregistered, Board::new().moves());
    }

    #[test]
    fn candidates_are_empty_without_a_root() {
        let book = Book::empty();
        assert!(collect_candidates(&book, &policy()).is_empty());
    }

    #[test]
    fn candidates_stop_at_the_ply_limit() {
        let mut book = Book::new();
        let mut board = Board::new();
        for _ in 0..3 {
            board = board.make_move(1u64 << board.moves().trailing_zeros());
            book.insert(&board, BookValue::exact(0));
        }
        let mut p = policy();
        p.max_ply = 2;
        let candidates = collect_candidates(&book, &p);
        assert!(candidates.iter().all(|c| (c.id.ply as usize) < 2));
    }

    #[test]
    fn candidates_prefer_low_error_then_high_uncertainty() {
        let mut a = Candidate {
            id: PositionId { ply: 5, slot: 0 },
            board: Board::new(),
            unregistered: 1,
            error: 0,
            uncertainty: 2,
        };
        let mut b = a;
        b.error = 4;
        assert!(a.priority() < b.priority(), "損の小さい方が先");

        b.error = 0;
        b.uncertainty = 40;
        assert!(b.priority() < a.priority(), "不確かな方が先");

        a.uncertainty = 40;
        a.id.ply = 2;
        assert!(a.priority() < b.priority(), "同じなら浅い方が先");
    }

    #[test]
    fn the_opponent_error_budget_opens_extra_lines() {
        // D3 の後の 3 通りに、値の差を付けて登録する。
        let mut book = Book::new();
        let root = Board::new().make_move(1u64 << D3);
        book.insert(&root, BookValue::exact(0));
        let mut legal = root.moves();
        let mut score = 0i8;
        while legal != 0 {
            let mv = legal.trailing_zeros();
            legal &= legal - 1;
            book.insert(&root.make_move(1u64 << mv), BookValue::exact(score));
            score -= 3;
        }
        book.propagate();

        // root は ply 1 = 相手の手番。opponent_error を広げると候補が増える。
        let mut narrow = policy();
        narrow.max_ply = 10;
        narrow.opponent_error = 0;
        let n_narrow = collect_candidates(&book, &narrow).len();

        let mut wide = narrow.clone();
        wide.opponent_error = 8;
        let n_wide = collect_candidates(&book, &wide).len();

        assert!(
            n_wide > n_narrow,
            "相手の損を許すと候補が増えるはず ({n_narrow} -> {n_wide})"
        );
    }

    #[test]
    fn fix_frontiers_marks_an_exhausted_position() {
        let mut book = Book::empty();
        let root = Board::new();
        book.insert(&root, BookValue::exact(0));
        // 4 手すべての子を登録する (どれも同じ正規形)。
        book.insert(&root.make_move(1u64 << D3), BookValue::exact(0));
        // frontier が「掘れる手がある」と言っているが実際には無い。
        book.set_frontier(
            &root,
            Frontier {
                mv: D3 as i8,
                score: 0,
                depth: 4,
            },
        );

        assert_eq!(fix_frontiers(&mut book), 1);
        assert!(book.frontier_of(&root).unwrap().is_exhausted());
    }

    #[test]
    fn fix_frontiers_clears_a_move_that_is_already_registered() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        let mv = root.moves().trailing_zeros() as u8;
        book.insert(&root, BookValue::exact(0));
        book.insert(&root.make_move(1u64 << mv), BookValue::exact(0));
        book.set_frontier(
            &root,
            Frontier {
                mv: mv as i8,
                score: 0,
                depth: 4,
            },
        );

        assert_eq!(fix_frontiers(&mut book), 1);
        assert!(book.frontier_of(&root).unwrap().is_unset());
    }

    #[test]
    fn fix_frontiers_leaves_a_consistent_book_alone() {
        let mut book = Book::new();
        book.set_frontier(&Board::new(), Frontier::unset());
        assert_eq!(fix_frontiers(&mut book), 0);
    }

    #[test]
    fn add_line_registers_the_positions_of_a_record() {
        let mut book = Book::empty();
        // F5 D6 C3 の 3 手。
        let record = [37u8, 43, 18];
        assert_eq!(add_line(&mut book, &record, 10), 3);
        assert_eq!(book.len(), 3);
        assert_eq!(book.max_ply(), Some(3));
    }

    #[test]
    fn add_line_stops_at_the_ply_limit() {
        let mut book = Book::empty();
        let record = [37u8, 43, 18];
        assert_eq!(add_line(&mut book, &record, 2), 2);
    }

    #[test]
    fn add_line_stops_at_an_illegal_move() {
        let mut book = Book::empty();
        // A1 は初手として合法ではない。
        assert_eq!(add_line(&mut book, &[0u8], 10), 0);
    }

    #[test]
    fn pinned_positions_are_never_overwritten_by_growth() {
        let mut book = Book::empty();
        let board = Board::new();
        let mut pinned = BookValue::exact(12);
        pinned.flags = pinned.flags.union(ValueFlags::PINNED);
        book.set_value(&board, pinned);
        book.insert(&board.make_move(1u64 << D3), BookValue::exact(0));
        book.propagate();
        assert_eq!(book.value_of(&board).unwrap().score, 12);
    }

    /// 既定の評価器で最小構成の solver を作る。
    fn test_solver() -> Solver {
        use crate::eval::evaluator::Evaluator;
        use crate::search::SolverOptions;
        Solver::with_options(
            std::sync::Arc::new(Evaluator::default()),
            SolverOptions::default(),
        )
    }

    #[test]
    fn growing_the_root_marks_it_exhausted_in_one_step() {
        // 初期盤面の 4 手はすべて同じ正規形に落ちるので、1 手を book に
        // 入れた時点で残りの 3 手も登録済みになる。frontier がそれを
        // 取りこぼすと「まだ掘れる手がある」と言い続けてしまう。
        let mut book = Book::new();
        let policy = GrowthPolicy {
            level: 1,
            max_ply: 4,
            max_positions: 1,
            ..GrowthPolicy::default()
        };
        let report = grow(&mut book, &test_solver(), &policy);
        assert_eq!(report.added, 1);

        let root = Board::new();
        assert_eq!(book.unregistered_moves(&root), 0);
        assert!(
            book.frontier_of(&root).unwrap().is_exhausted(),
            "frontier: {:?}",
            book.frontier_of(&root)
        );
        // 上界が閉じるので、根の値が確定幅で出る。
        let value = book.value_of(&root).unwrap();
        assert!(value.upper < SCORE_MAX, "upper = {}", value.upper);
    }

    #[test]
    fn growth_stops_at_the_position_budget() {
        let mut book = Book::new();
        let policy = GrowthPolicy {
            level: 1,
            max_ply: 6,
            max_positions: 3,
            ..GrowthPolicy::default()
        };
        let report = grow(&mut book, &test_solver(), &policy);
        assert_eq!(report.added, 3);
        assert_eq!(report.stopped_by, StopReason::PositionBudget);
        assert_eq!(book.len(), 4);
    }

    #[test]
    fn growth_stops_when_interrupted() {
        let mut book = Book::new();
        let stop = std::sync::Arc::new(AtomicBool::new(true));
        let policy = GrowthPolicy {
            level: 1,
            max_positions: 10,
            stop: Some(stop),
            ..GrowthPolicy::default()
        };
        let report = grow(&mut book, &test_solver(), &policy);
        assert_eq!(report.stopped_by, StopReason::Interrupted);
        assert_eq!(report.added, 0);
    }

    #[test]
    fn growth_records_the_policy_in_the_metadata() {
        let mut book = Book::new();
        let policy = GrowthPolicy {
            level: 3,
            max_ply: 7,
            player_error: 1,
            opponent_error: 5,
            max_positions: 1,
            ..GrowthPolicy::default()
        };
        grow(&mut book, &test_solver(), &policy);
        assert_eq!(book.meta().level, 3);
        assert_eq!(book.meta().max_ply, 7);
        assert_eq!(book.meta().player_error, 1);
        assert_eq!(book.meta().opponent_error, 5);
    }

    #[test]
    fn search_shape_reports_a_full_read_for_the_endgame() {
        assert_eq!(search_shape(&SolverType::Final(6), 20), (20, 6));
        assert_eq!(search_shape(&SolverType::Eval(12, 3), 40), (12, 3));
    }
}
