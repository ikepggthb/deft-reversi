//! 定石 book。
//!
//! ```no_run
//! use deft_reversi_engine::{Board, Book};
//!
//! let book = Book::load("book.dbk").unwrap();
//! if let Some(probe) = book.probe(&Board::new()) {
//!     println!("{:+} ({})", probe.value.score, probe.trust());
//!     for mv in &probe.moves {
//!         println!("  {} {:+}", mv.mv, mv.value.score);
//!     }
//! }
//! ```
//!
//! # 設計
//!
//! Edax と Egaroucid の book を一通り読んだうえで、この book は 3 点を変えた。
//!
//! ## 1. 値ではなく「値と、その値がどれだけ信用できるか」を持つ
//!
//! Edax は評価値と上下界を、Egaroucid は評価値と探索レベルだけを持つ。
//! どちらも「この値はどこまで詰めた結果なのか」をファイルから復元できない
//! (level はそのエンジンの探索表に紐づく数字なので、エンジンをまたぐと
//! 意味が変わる)。
//!
//! ここでは [`BookValue`] が探索の中身そのもの (深さ・選択度・完全読みか) と
//! 上下界を持つ。上下界は子から積み上げて計算するので、
//!
//! - 「登録済みの手の中で最善」が **下界**
//! - 「まだ登録していない手 ([`Frontier`]) を含めた最善の見積もり」が **上界**
//!
//! になる。この幅 ([`BookValue::uncertainty`]) がそのまま
//!
//! - 育成でどこを掘るべきか ([`grow`])
//! - 対局中に book を信じてよいか ([`Probe::trust`])
//!
//! の判断材料になる。Edax の上下界は探索の窓をそのまま保存したもので、
//! 子から積み上げてはいない。
//!
//! ## 2. 局面を手数 (ply) の層に分けて持つ
//!
//! 子は必ず 1 手多いので、層に分けると
//!
//! - 評価値の伝播が「深い層から順に舐めるだけ」の線形走査になる
//!   (木を辿る必要も訪問済み判定も要らない)
//! - 同じ層の中は互いに独立なので、そのまま並列に処理できる
//! - 子を引くのは常に次の層だけなので、参照の局所性が良い
//!
//! 詳しくは [`layer`]。
//!
//! ## 3. 定石名を持つ
//!
//! Edax にも Egaroucid にも無い。対局中に「この進行は 虎 です」と出せる。
//! 詳しくは [`names`]。
//!
//! # ファイル形式
//!
//! 標準は独自形式の `.dbk` ([`dbk`])。Egaroucid の `.egbk3` ([`egbk`]) と
//! Edax の `book.dat` ([`edax`]) は取り込み・書き出しで対応する。

pub mod dbk;
pub mod edax;
pub mod egbk;
pub mod grow;
pub mod layer;
pub mod names;
mod random;
pub mod sym;
pub mod value;

pub use dbk::DBK_MAGIC;
pub use edax::EDAX_BOOK_MAGIC;
pub use egbk::{EGBK_MAGIC, EGBK_VERSION};
pub use grow::{add_line, fix_frontiers, grow, GrowthPolicy, GrowthReport, StopReason};
pub use layer::{ply_of, LayeredTable, PlyLayer, PositionId, MAX_PLY};
pub use names::{NameTable, Opening};
pub use sym::{
    convert_coord_from_representative, convert_coord_to_representative, representative_board,
};
pub use value::{
    clamp_score, is_valid_move, is_valid_score, search_error, BookValue, Frontier, ValueFlags,
    DEPTH_UNKNOWN, MOVE_NONE, MOVE_PASS, SCORE_MAX, SCORE_UNDEFINED, SELECTIVITY_EXACT,
};

use crate::board::board::Board;
use crate::EngineError;
use random::Random;

/// book の手が本譜の評価値からこれ以上離れていたら book を使わない。
///
/// book が自己矛盾している (伝播漏れなど) ときの保険。
pub const BOOK_LOSS_IGNORE_THRESHOLD: i32 = 8;

/// [`PickPolicy::from_accuracy_level`] が受け取る精度レベルの上限。
///
/// Egaroucid の `BOOK_ACCURACY_LEVEL_INF` と同じ 0..=10 の目盛りで、
/// 既存の CLI 引数との互換のために残してある。
pub const BOOK_ACCURACY_LEVEL_INF: i32 = 10;

/// book の値がどれだけ信用できるか。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trust {
    /// 値が無い。
    None,
    /// 幅が広い。探索し直した方がよい。
    Low,
    /// 幅が狭い。そのまま使ってよい。
    High,
    /// 終局まで読み切った確定値。
    Exact,
}

impl Trust {
    /// 上下界の幅から求める。
    pub fn of(value: &BookValue) -> Self {
        if !value.is_defined() {
            return Trust::None;
        }
        if value.is_exact() {
            return Trust::Exact;
        }
        if value.uncertainty() <= 4 {
            Trust::High
        } else {
            Trust::Low
        }
    }
}

impl std::fmt::Display for Trust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Trust::None => "none",
            Trust::Low => "low",
            Trust::High => "high",
            Trust::Exact => "exact",
        };
        f.write_str(s)
    }
}

/// book に登録されている手 1 つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookMove {
    /// 問い合わせた盤面の向きでの座標 (0..64)。
    pub mv: u8,
    /// 打った側の手番から見た値。
    pub value: BookValue,
}

/// book の由来と育成の設定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookMeta {
    /// 育成に使った探索レベル。
    pub level: u8,
    /// 育成の対象にする最大 ply。0 なら制限なし。
    pub max_ply: u8,
    /// 自分の手で許す損 (石差)。
    pub player_error: u8,
    /// 相手の手で許す損 (石差)。
    pub opponent_error: u8,
    /// 由来を人が読める形で残す欄。
    pub note: String,
}

/// 局面を 1 つ引いた結果。対局中に必要な情報をまとめて返す。
#[derive(Clone, Debug)]
pub struct Probe<'a> {
    /// この局面自身の値。
    pub value: BookValue,
    /// book に登録されている手。値の良い順。
    pub moves: Vec<BookMove>,
    /// この局面が終点になる定石の名前。
    pub names: Vec<&'a str>,
    /// この局面を通る定石の名前。
    pub upcoming: Vec<&'a str>,
    /// 合法手をすべて book に持っているか。
    pub complete: bool,
}

impl Probe<'_> {
    /// 値の信用度。
    pub fn trust(&self) -> Trust {
        Trust::of(&self.value)
    }

    /// 最善の手。
    pub fn best(&self) -> Option<BookMove> {
        self.moves.first().copied()
    }

    /// book と自分の値が食い違っていないか。
    ///
    /// 子の最善が自分の値より [`BOOK_LOSS_IGNORE_THRESHOLD`] 以上悪いときは、
    /// 伝播漏れなどで book が壊れている。
    pub fn is_consistent(&self) -> bool {
        let Some(best) = self.moves.first() else {
            return true;
        };
        if !self.value.is_defined() {
            return true;
        }
        best.value.score as i32 >= self.value.score as i32 - BOOK_LOSS_IGNORE_THRESHOLD
    }
}

/// 対局中に book から手を選ぶときの方針。
///
/// Egaroucid の `acc_level` は「指数を変えて重みを歪める」という数字で
/// 意味が読み取りにくいので、ここは
///
/// - どれだけ損を許すか (`max_loss`)
/// - どれだけばらけさせるか (`randomness`)
/// - どれだけ信用できる値でないと使わないか (`min_trust`)
///
/// の 3 つに分けてある。
#[derive(Clone, Copy, Debug)]
pub struct PickPolicy {
    /// 最善手からこれ以上悪い手は選ばない (石差)。
    pub max_loss: i32,
    /// 0.0 で常に最善手、1.0 で許容範囲から一様に選ぶ。
    pub randomness: f64,
    /// これ未満の信用度しか無いときは book を使わない。
    pub min_trust: Trust,
    /// 選んでよい手のビットマスク。
    pub allowed: u64,
}

impl Default for PickPolicy {
    fn default() -> Self {
        Self {
            max_loss: 0,
            randomness: 0.0,
            min_trust: Trust::Low,
            allowed: !0u64,
        }
    }
}

impl PickPolicy {
    /// 最善手だけを選ぶ方針。
    pub fn best() -> Self {
        Self::default()
    }

    /// Egaroucid の精度レベル (0..=10) に近い方針を作る。
    ///
    /// 0 が最善手のみ、大きいほど広く・ばらけて選ぶ。
    pub fn from_accuracy_level(acc_level: i32) -> Self {
        let acc = acc_level.clamp(0, BOOK_ACCURACY_LEVEL_INF);
        Self {
            max_loss: 2 * acc,
            randomness: acc as f64 / BOOK_ACCURACY_LEVEL_INF as f64,
            min_trust: Trust::Low,
            allowed: !0u64,
        }
    }

    /// 選んでよい手を絞る。
    pub fn with_allowed(mut self, allowed: u64) -> Self {
        self.allowed = allowed;
        self
    }
}

/// 参照した子局面 1 つ分。
#[derive(Clone, Copy, Debug)]
struct Child {
    /// 親の盤面での着手座標。
    mv: u8,
    /// 子局面の場所。
    id: PositionId,
    /// 子の値に掛ける符号。パスを挟むと +1 になる。
    sign: i8,
}

/// book に入っている子局面 1 つ。
#[derive(Clone, Copy, Debug)]
pub struct ChildRef {
    /// 親の盤面の向きでの着手座標。
    pub mv: u8,
    /// 子局面の場所。
    pub id: PositionId,
    /// 手番が入れ替わるか。相手がパスする手では `false`。
    pub flips_turn: bool,
    /// 親の手番から見た子の値。
    pub value: BookValue,
}

/// 定石 book。
#[derive(Clone, Debug)]
pub struct Book {
    table: LayeredTable,
    names: NameTable,
    meta: BookMeta,
    random: Random,
}

impl Default for Book {
    fn default() -> Self {
        Self::new()
    }
}

impl Book {
    /// 初期盤面だけを含む book を作る。
    pub fn new() -> Self {
        let mut book = Self::empty();
        book.table
            .insert(Board::new(), BookValue::undefined(), Frontier::unset());
        book
    }

    /// 局面を 1 つも持たない book を作る。
    pub fn empty() -> Self {
        Self {
            table: LayeredTable::new(),
            names: NameTable::default(),
            meta: BookMeta::default(),
            random: Random::from_clock(),
        }
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn table(&self) -> &LayeredTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut LayeredTable {
        &mut self.table
    }

    pub fn meta(&self) -> &BookMeta {
        &self.meta
    }

    pub fn meta_mut(&mut self) -> &mut BookMeta {
        &mut self.meta
    }

    pub fn names(&self) -> &NameTable {
        &self.names
    }

    pub fn names_mut(&mut self) -> &mut NameTable {
        &mut self.names
    }

    /// 定石名の表を差し替える。
    pub fn set_names(&mut self, names: NameTable) {
        self.names = names;
    }

    /// ランダム手選択の乱数種を固定する (テスト・再現用)。
    pub fn set_random_seed(&mut self, seed: u64) {
        self.random = Random::new(seed);
    }

    /// 局面が入っている最も深い ply。
    pub fn max_ply(&self) -> Option<usize> {
        self.table.max_ply()
    }

    /// 全局面を ply の浅い順に走査する。
    pub fn iter(&self) -> impl Iterator<Item = (PositionId, &Board, &BookValue)> {
        self.table.iter()
    }

    /// まだ値が入っていない局面の数。
    ///
    /// 棋譜から局面だけ足した直後はここが大きい。Egaroucid / Edax 形式は
    /// 「値が無い」を表せないので、書き出しではこの分が落ちる。
    pub fn n_undefined(&self) -> usize {
        self.iter().filter(|(_, _, v)| !v.is_defined()).count()
    }

    // ---- 局面の出し入れ ----

    /// 盤面 (対称形を含む) が登録されているか。
    pub fn contains(&self, board: &Board) -> bool {
        self.table.locate(board).is_some()
    }

    /// 盤面 (対称形を含む) の値。
    ///
    /// 値は手番から見た石差なので、向きによらず同じ。
    pub fn value_of(&self, board: &Board) -> Option<BookValue> {
        self.table.locate(board).map(|id| *self.table.value(id))
    }

    /// 盤面 (対称形を含む) の frontier を、**`board` の向きの座標で**返す。
    pub fn frontier_of(&self, board: &Board) -> Option<Frontier> {
        let ply = ply_of(board);
        let (rep, idx) = representative_board(board);
        let id = self.table.locate_representative(ply, &rep)?;
        let mut frontier = *self.table.frontier(id);
        if frontier.has_move() {
            frontier.mv = convert_coord_from_representative(frontier.mv as u8, idx) as i8;
        }
        Some(frontier)
    }

    /// 局面を登録する。既にあれば値を [`Book::update`] と同じ規則で取り込む。
    /// 新しく追加したときだけ `true`。
    pub fn insert(&mut self, board: &Board, value: BookValue) -> bool {
        let (rep, _) = representative_board(board);
        let before = self.table.len();
        let id = self.table.insert(rep, value, Frontier::unset());
        if self.table.len() != before {
            return true;
        }
        let current = self.table.value_mut(id);
        if current.is_superseded_by(&value) {
            *current = value;
        }
        false
    }

    /// 値を取り込む。より信用できる方を残す。
    pub fn update(&mut self, board: &Board, value: BookValue) {
        self.insert(board, value);
    }

    /// 値を無条件に置き換える (無ければ登録する)。
    pub fn set_value(&mut self, board: &Board, value: BookValue) {
        let (rep, _) = representative_board(board);
        let id = self.table.insert(rep, value, Frontier::unset());
        *self.table.value_mut(id) = value;
    }

    /// frontier を差し替える。座標は正規形の向きに移して保存する。
    pub fn set_frontier(&mut self, board: &Board, frontier: Frontier) {
        let (rep, idx) = representative_board(board);
        let mut frontier = frontier;
        if frontier.has_move() {
            frontier.mv = convert_coord_to_representative(frontier.mv as u8, idx) as i8;
        }
        let id = self
            .table
            .insert(rep, BookValue::undefined(), Frontier::unset());
        *self.table.frontier_mut(id) = frontier;
    }

    /// 全局面を消して初期盤面だけに戻す。定石名は残す。
    pub fn clear(&mut self) {
        self.table.clear();
        self.table
            .insert(Board::new(), BookValue::undefined(), Frontier::unset());
    }

    /// 別の book の局面をすべて取り込む。追加された局面数を返す。
    pub fn merge(&mut self, other: &Book) -> usize {
        let mut added = 0;
        for ply in 0..=other.table.max_ply().unwrap_or(0) {
            let layer = other.table.layer(ply);
            for slot in 0..layer.len() as u32 {
                let board = *layer.board(slot);
                let value = *layer.value(slot);
                let frontier = *layer.frontier(slot);

                let before = self.table.len();
                let id = self.table.insert(board, value, frontier);
                if self.table.len() != before {
                    added += 1;
                    continue;
                }
                let current = self.table.value_mut(id);
                if current.is_superseded_by(&value) {
                    *current = value;
                }
                let current = self.table.frontier_mut(id);
                if current.is_unset() {
                    *current = frontier;
                }
            }
        }
        for opening in other.names.openings() {
            let _ = self.names.insert(&opening.name, &opening.record);
        }
        added
    }

    // ---- 子局面の参照 ----
    //
    // link を保存しない。合法手を実際に打ってみて、その子局面が book に
    // あるかを次の層から引く。子は必ず ply + 1 なので、探す層は 1 つだけ。

    /// `board` の子で book にあるものを 1 つずつ渡す。
    fn for_each_child(&self, board: &Board, mut f: impl FnMut(Child)) {
        let ply = ply_of(board);
        if ply + 1 >= layer::N_LAYERS {
            return;
        }
        let layer = self.table.layer(ply + 1);
        if layer.is_empty() {
            return;
        }
        let child_ply = (ply + 1) as u8;

        let mut legal = board.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            let child = board.make_move(1u64 << mv);

            let child_legal = child.moves();
            if child_legal == 0 && child.opponent_moves() == 0 {
                // 終局。どちらの向きで登録されているか分からないので両方見る。
                if let Some(slot) = layer.slot_of(&child.unique_board()) {
                    f(Child {
                        mv,
                        id: PositionId {
                            ply: child_ply,
                            slot,
                        },
                        sign: -1,
                    });
                    continue;
                }
                if let Some(slot) = layer.slot_of(&child.passed().unique_board()) {
                    f(Child {
                        mv,
                        id: PositionId {
                            ply: child_ply,
                            slot,
                        },
                        sign: 1,
                    });
                }
            } else if child_legal == 0 {
                // 相手がパスするので手番が戻る = 符号は反転しない。
                if let Some(slot) = layer.slot_of(&child.passed().unique_board()) {
                    f(Child {
                        mv,
                        id: PositionId {
                            ply: child_ply,
                            slot,
                        },
                        sign: 1,
                    });
                }
            } else if let Some(slot) = layer.slot_of(&child.unique_board()) {
                f(Child {
                    mv,
                    id: PositionId {
                        ply: child_ply,
                        slot,
                    },
                    sign: -1,
                });
            }
        }
    }

    /// 子局面が book にある手のビットマスク。
    pub fn registered_moves(&self, board: &Board) -> u64 {
        let mut mask = 0u64;
        self.for_each_child(board, |child| mask |= 1u64 << child.mv);
        mask
    }

    /// まだ book に無い合法手のビットマスク。育成の候補。
    pub fn unregistered_moves(&self, board: &Board) -> u64 {
        board.moves() & !self.registered_moves(board)
    }

    /// book に入っている子局面の一覧。値の良い順。
    ///
    /// 値が入っていない子も返す (育成中は場所だけ確保された局面がありうる)。
    pub fn children(&self, board: &Board) -> Vec<ChildRef> {
        let mut out = Vec::new();
        self.for_each_child(board, |child| {
            let raw = *self.table.value(child.id);
            out.push(ChildRef {
                mv: child.mv,
                id: child.id,
                flips_turn: child.sign < 0,
                value: if child.sign < 0 { raw.negated() } else { raw },
            });
        });
        out.sort_by(|a, b| b.value.score.cmp(&a.value.score));
        out
    }

    /// book に登録されている手と値を、`board` の向きで返す。値の良い順。
    pub fn moves(&self, board: &Board) -> Vec<BookMove> {
        self.children(board)
            .into_iter()
            .filter(|c| c.value.is_defined())
            .map(|c| BookMove {
                mv: c.mv,
                value: c.value,
            })
            .collect()
    }

    /// 局面を引く。book に何も無ければ `None`。
    pub fn probe(&self, board: &Board) -> Option<Probe<'_>> {
        let moves = self.moves(board);
        let own = self.value_of(board);
        if moves.is_empty() && own.is_none() {
            return None;
        }
        let registered = self.registered_moves(board);
        Some(Probe {
            value: own.unwrap_or_else(BookValue::undefined),
            complete: board.moves() == 0 || registered == board.moves(),
            moves,
            names: self.names.names_at(board),
            upcoming: self.names.names_through(board),
        })
    }

    /// book の最善手。
    pub fn best_move(&self, board: &Board) -> Option<BookMove> {
        self.moves(board).into_iter().next()
    }

    /// 方針に従って手を 1 つ選ぶ。
    ///
    /// - 値が `policy.min_trust` に届かなければ `None` (探索に任せる)
    /// - book が自己矛盾していれば `None`
    pub fn pick(&mut self, board: &Board, policy: &PickPolicy) -> Option<BookMove> {
        let probe = self.probe(board)?;
        if !probe.is_consistent() {
            return None;
        }

        let mut moves: Vec<BookMove> = probe
            .moves
            .iter()
            .copied()
            .filter(|m| policy.allowed & (1u64 << m.mv) != 0)
            .collect();
        if moves.is_empty() {
            return None;
        }

        let best = moves[0].value.score as i32;
        if Trust::of(&moves[0].value) < policy.min_trust {
            return None;
        }
        moves.retain(|m| (best - m.value.score as i32) <= policy.max_loss);

        if policy.randomness <= 0.0 || moves.len() == 1 {
            return Some(moves[0]);
        }

        // 損の小さい手ほど重い。randomness = 1 で一様、0 に近づくほど最善に寄る。
        let sharpness = (1.0 - policy.randomness.clamp(0.0, 1.0)) * 4.0;
        let weights: Vec<f64> = moves
            .iter()
            .map(|m| {
                let loss = (best - m.value.score as i32) as f64;
                (-sharpness * loss).exp()
            })
            .collect();
        let sum: f64 = weights.iter().sum();
        if sum <= 0.0 || !sum.is_finite() {
            return Some(moves[0]);
        }

        let mut target = self.random.next_f64() * sum;
        for (m, w) in moves.iter().zip(&weights) {
            target -= w;
            if target <= 0.0 {
                return Some(*m);
            }
        }
        moves.last().copied()
    }

    /// `board` から book を辿って得られる手順。
    pub fn line(&mut self, board: &Board, policy: &PickPolicy) -> Vec<u8> {
        let mut line = Vec::new();
        let mut board = *board;
        let mut policy = *policy;
        loop {
            if board.moves() == 0 {
                if board.opponent_moves() == 0 {
                    break;
                }
                board = board.passed();
                continue;
            }
            policy.allowed = !0u64;
            let Some(mv) = self.pick(&board, &policy) else {
                break;
            };
            line.push(mv.mv);
            board = board.make_move(1u64 << mv.mv);
            if line.len() >= MAX_PLY {
                break;
            }
        }
        line
    }

    // ---- 値の伝播 ----

    /// 子から親へ値を積み上げる。
    ///
    /// 深い層から順に 1 回舐めるだけで済む。子は必ず 1 つ深い層にいるので、
    /// 層を降りきった時点でその層の子はすべて確定している。
    ///
    /// 積み上げる値は
    ///
    /// - **下界** = 登録済みの子の中の最善 (その手は実際に選べる)
    /// - **上界** = 合法手をすべて持っていれば同じく子から、持っていなければ
    ///   frontier の見積もりも入れた値。frontier を調べていなければ上界不明
    ///
    /// になる。この幅が [`BookValue::uncertainty`] で、育成と対局の両方で
    /// 判断の材料になる。
    ///
    /// 書き換えた局面数を返す。
    pub fn propagate(&mut self) -> usize {
        let Some(max_ply) = self.table.max_ply() else {
            return 0;
        };
        let mut changed = 0;
        for ply in (0..=max_ply).rev() {
            let n = self.table.layer(ply).len();
            for slot in 0..n as u32 {
                let id = PositionId {
                    ply: ply as u8,
                    slot,
                };
                if self.table.value(id).is_pinned() {
                    continue;
                }
                let board = *self.table.board(id);
                let Some(backed) = self.backed_up_value(&board, id) else {
                    continue;
                };
                let current = self.table.value(id);
                // 同じ信用度なら積み上げた値を採る (実際の続きを見ている分だけ確か)。
                if backed.is_superseded_by(current) {
                    continue;
                }
                if *current != backed {
                    *self.table.value_mut(id) = backed;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// 登録済みの子から積み上げた値。子が 1 つも無ければ `None`。
    fn backed_up_value(&self, board: &Board, id: PositionId) -> Option<BookValue> {
        let n_empties = board.empties_count() as u8;

        let mut best_score = SCORE_UNDEFINED;
        let mut lower = -SCORE_MAX;
        let mut upper_from_children = -SCORE_MAX;
        let mut depth = 0u8;
        let mut selectivity = value::SELECTIVITY_EXACT;
        let mut registered = 0u64;
        let mut n_children = 0usize;

        self.for_each_child(board, |child| {
            let raw = *self.table.value(child.id);
            if !raw.is_defined() {
                return;
            }
            let v = if child.sign < 0 { raw.negated() } else { raw };
            registered |= 1u64 << child.mv;
            n_children += 1;
            if v.score > best_score || best_score == SCORE_UNDEFINED {
                best_score = v.score;
                depth = v.depth.saturating_add(1);
                selectivity = v.selectivity;
            }
            lower = lower.max(v.lower);
            upper_from_children = upper_from_children.max(v.upper);
        });

        if n_children == 0 {
            return None;
        }

        let legal = board.moves();
        let complete = legal == 0 || registered == legal;
        let upper = if complete {
            upper_from_children
        } else {
            match self.table.frontier(id).upper_estimate(n_empties) {
                Some(est) => upper_from_children.max(est),
                None => SCORE_MAX,
            }
        };

        let mut value = BookValue::with_bounds(best_score, lower, upper, depth, selectivity);
        value.flags = value.flags.union(ValueFlags::PROPAGATED);
        Some(value)
    }

    /// 初期盤面から辿れない局面を削除する。削除した数を返す。
    ///
    /// 層構造のおかげで、浅い層から順に「親が生きているか」を見るだけで済む。
    /// 木を辿る必要も訪問済みの集合も要らない。
    pub fn prune_unreachable(&mut self) -> usize {
        let Some(max_ply) = self.table.max_ply() else {
            return 0;
        };
        let root = Board::new().unique_board();

        // ply 0 は初期盤面だけを残す。
        let mut removed = self.table.layer_mut(0).retain(|board, _| *board == root);

        for ply in 0..max_ply {
            // 生きている親から届く子を集める。
            let mut reachable: Vec<Board> = Vec::new();
            let parents = self.table.layer(ply);
            for slot in 0..parents.len() as u32 {
                let board = *parents.board(slot);
                let mut boards = [board, board.passed()];
                if board.moves() != 0 {
                    boards[1] = board;
                }
                for parent in boards {
                    let mut legal = parent.moves();
                    while legal != 0 {
                        let mv = legal.trailing_zeros();
                        legal &= legal - 1;
                        let child = parent.make_move(1u64 << mv);
                        reachable.push(child.unique_board());
                        if child.moves() == 0 {
                            reachable.push(child.passed().unique_board());
                        }
                    }
                }
            }
            reachable.sort_unstable_by_key(|b| (b.player, b.opponent));
            reachable.dedup();
            removed += self.table.layer_mut(ply + 1).retain(|board, _| {
                reachable
                    .binary_search_by_key(&(board.player, board.opponent), |b| {
                        (b.player, b.opponent)
                    })
                    .is_ok()
            });
        }
        removed
    }

    /// 最善から離れた変化を削る。
    ///
    /// 親から見て `max_loss` を超えて損をする手の先を消す。値がまだ入って
    /// いない子は判断できないので残す。削除した数を返す。
    pub fn reduce(&mut self, max_loss: i32) -> usize {
        let Some(max_ply) = self.table.max_ply() else {
            return 0;
        };
        // 浅い層から「この局面は残すか」を決め、残す局面から届く子だけを残す。
        let root = Board::new().unique_board();
        let mut removed = self.table.layer_mut(0).retain(|board, _| *board == root);

        for ply in 0..max_ply {
            let mut keep = vec![false; self.table.layer(ply + 1).len()];
            for slot in 0..self.table.layer(ply).len() as u32 {
                let board = *self.table.layer(ply).board(slot);
                let children = self.children(&board);
                let best = children
                    .iter()
                    .filter(|c| c.value.is_defined())
                    .map(|c| c.value.score as i32)
                    .max();
                for child in &children {
                    let within = match (best, child.value.is_defined()) {
                        (Some(best), true) => best - child.value.score as i32 <= max_loss,
                        // 値が分からない子は判断できないので残す。
                        _ => true,
                    };
                    if within {
                        keep[child.id.slot as usize] = true;
                    }
                }
            }
            let mut slot = 0usize;
            removed += self.table.layer_mut(ply + 1).retain(|_, _| {
                let k = keep[slot];
                slot += 1;
                k
            });
        }
        removed
    }

    // ---- 読み書き ----

    /// book ファイルを拡張子と中身のマジックから判別して読み込む。
    ///
    /// `.dbk` / `.egbk3` / `.egbk2` / `.egbk` / Edax の `.dat` に対応する。
    pub fn load(path: &str) -> Result<Self, EngineError> {
        // 大きくなりうる形式はストリームで読み、ファイル全体をメモリに載せない。
        if let Some(result) = Self::load_dbk_streaming(path) {
            return result;
        }
        if let Some(result) = Self::load_egbk3_streaming(path) {
            return result;
        }
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// バイト列の先頭を見て形式を判別し読み込む。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        if bytes.starts_with(dbk::DBK_MAGIC) {
            return Self::from_dbk_bytes(bytes);
        }
        if bytes.starts_with(egbk::EGBK_MAGIC) {
            return Self::from_egbk_bytes(bytes);
        }
        if bytes.len() >= 8 && &bytes[0..8] == edax::EDAX_BOOK_MAGIC {
            return Self::from_edax_bytes(bytes);
        }
        // マジックの無い最初期の .egbk。
        Self::from_egbk1_bytes(bytes, 1)
    }

    /// 標準形式 (`.dbk`) で保存する。
    pub fn save(&self, path: &str) -> Result<(), EngineError> {
        self.save_dbk(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u8 = 19;

    /// 初期盤面 -> D3 -> (最初の合法手) の 3 局面を持つ book。
    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        let grandchild = child.make_move(1u64 << child.moves().trailing_zeros());

        book.insert(&root, BookValue::exact(2));
        book.insert(&child, BookValue::exact(-2));
        book.insert(&grandchild, BookValue::exact(3));
        book.set_random_seed(1);
        book
    }

    #[test]
    fn new_book_contains_only_the_initial_position() {
        let book = Book::new();
        assert_eq!(book.len(), 1);
        assert!(book.contains(&Board::new()));
    }

    #[test]
    fn positions_land_in_the_layer_of_their_move_count() {
        let book = sample_book();
        assert_eq!(book.table().layer(0).len(), 1);
        assert_eq!(book.table().layer(1).len(), 1);
        assert_eq!(book.table().layer(2).len(), 1);
        assert_eq!(book.max_ply(), Some(2));
    }

    #[test]
    fn contains_finds_every_symmetric_board() {
        let book = sample_book();
        let child = Board::new().make_move(1u64 << D3);
        for sym in child.all_symmetries() {
            assert!(book.contains(&sym), "missing symmetry of D3");
        }
    }

    #[test]
    fn moves_are_derived_from_child_positions() {
        let book = sample_book();
        let moves = book.moves(&Board::new());
        // 初期盤面の 4 手はすべて同じ正規形に落ちるので 4 手とも返る。
        assert_eq!(moves.len(), 4);
        for m in &moves {
            assert_ne!(Board::new().moves() & (1u64 << m.mv), 0);
            // 子の -2 が符号反転して +2 になる。
            assert_eq!(m.value.score, 2);
        }
    }

    #[test]
    fn moves_are_orientation_consistent() {
        let book = sample_book();
        for sym in Board::new().all_symmetries() {
            let moves = book.moves(&sym);
            assert_eq!(moves.len(), 4);
            for m in moves {
                assert_ne!(sym.moves() & (1u64 << m.mv), 0);
            }
        }
    }

    #[test]
    fn frontier_is_returned_in_the_queried_orientation() {
        let mut book = Book::empty();
        let board = Board::new().make_move(1u64 << D3);
        let mv = board.moves().trailing_zeros() as i8;
        book.set_frontier(
            &board,
            Frontier {
                mv,
                score: 3,
                depth: 10,
            },
        );

        for sym in board.all_symmetries() {
            let frontier = book.frontier_of(&sym).unwrap();
            assert!(frontier.has_move());
            assert_ne!(
                sym.moves() & (1u64 << frontier.mv),
                0,
                "frontier move {} is not legal on this symmetry",
                frontier.mv
            );
        }
    }

    #[test]
    fn probe_reports_names_and_completeness() {
        let mut book = sample_book();
        book.names_mut().insert("テスト定石", "F5").unwrap();

        let probe = book.probe(&Board::new()).unwrap();
        assert_eq!(probe.moves.len(), 4);
        assert!(probe.complete, "初期盤面の 4 手はすべて登録済み");
        assert_eq!(probe.trust(), Trust::Exact);

        let after_f5 = Board::new().make_move(1u64 << 37);
        let probe = book.probe(&after_f5).unwrap();
        assert_eq!(probe.names, vec!["テスト定石"]);
    }

    #[test]
    fn probe_is_none_for_an_unknown_position() {
        let book = Book::empty();
        assert!(book.probe(&Board::new()).is_none());
    }

    #[test]
    fn propagate_backs_the_child_value_up_to_the_parent() {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.insert(&root, BookValue::undefined());
        book.insert(&child, BookValue::exact(-6));

        assert_eq!(book.propagate(), 1);
        let value = book.value_of(&root).unwrap();
        assert_eq!(value.score, 6);
        assert!(value.is_exact(), "子が確定値なら親も確定値");
    }

    #[test]
    fn propagate_leaves_a_window_when_moves_are_missing() {
        let mut book = Book::empty();
        // D3 の 1 手だけを持たせる。他の 3 手は同じ正規形なので、
        // 中盤の局面を使って「登録されていない手がある」状況を作る。
        let root = Board::new().make_move(1u64 << D3);
        let mv = root.moves().trailing_zeros();
        let child = root.make_move(1u64 << mv);
        book.insert(&root, BookValue::undefined());
        book.insert(&child, BookValue::exact(2));

        book.propagate();
        let value = book.value_of(&root).unwrap();
        assert_eq!(value.lower, -2, "登録済みの手が下界を決める");
        assert_eq!(value.upper, SCORE_MAX, "未登録の手があるので上界は不明");
        assert!(!value.is_exact());
        assert_eq!(Trust::of(&value), Trust::Low);
    }

    #[test]
    fn propagate_closes_the_window_once_the_frontier_is_exhausted() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        let mv = root.moves().trailing_zeros();
        let child = root.make_move(1u64 << mv);
        book.insert(&root, BookValue::undefined());
        book.insert(&child, BookValue::exact(2));
        // 「他に掘る手はもう無い」と分かっている状態。
        book.set_frontier(&root, Frontier::none());

        book.propagate();
        let value = book.value_of(&root).unwrap();
        assert_eq!((value.lower, value.upper), (-2, -2));
        assert!(value.is_exact());
    }

    #[test]
    fn propagate_uses_the_frontier_estimate_for_the_upper_bound() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        let mv = root.moves().trailing_zeros();
        let child = root.make_move(1u64 << mv);
        book.insert(&root, BookValue::undefined());
        book.insert(&child, BookValue::exact(2));
        book.set_frontier(
            &root,
            Frontier {
                mv: root.moves().trailing_zeros() as i8,
                score: 0,
                depth: 20,
            },
        );

        book.propagate();
        let value = book.value_of(&root).unwrap();
        assert_eq!(value.lower, -2);
        assert!(
            value.upper < SCORE_MAX,
            "frontier を調べてあれば上界が抑えられる: {}",
            value.upper
        );
    }

    #[test]
    fn pinned_values_survive_propagation() {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        let mut pinned = BookValue::exact(0);
        pinned.flags = pinned.flags.union(ValueFlags::PINNED);
        book.set_value(&root, pinned);
        book.insert(&child, BookValue::exact(-6));

        book.propagate();
        assert_eq!(book.value_of(&root).unwrap().score, 0);
    }

    #[test]
    fn best_move_picks_the_highest_score() {
        let book = sample_book();
        assert_eq!(book.best_move(&Board::new()).unwrap().value.score, 2);
    }

    #[test]
    fn pick_returns_only_legal_registered_moves() {
        let mut book = sample_book();
        let policy = PickPolicy::from_accuracy_level(5);
        for _ in 0..50 {
            let mv = book.pick(&Board::new(), &policy).unwrap();
            assert_ne!(Board::new().moves() & (1u64 << mv.mv), 0);
        }
    }

    #[test]
    fn pick_respects_the_move_mask() {
        let mut book = sample_book();
        let policy = PickPolicy::from_accuracy_level(5).with_allowed(1u64 << D3);
        for _ in 0..20 {
            assert_eq!(book.pick(&Board::new(), &policy).unwrap().mv, D3);
        }
    }

    #[test]
    fn pick_refuses_a_book_that_contradicts_itself() {
        let mut book = sample_book();
        // 自分の値だけ大きく引き上げると、子との差が閾値を超えて None になる。
        book.set_value(
            &Board::new(),
            BookValue::exact(2 + BOOK_LOSS_IGNORE_THRESHOLD as i8 + 1),
        );
        assert!(book.pick(&Board::new(), &PickPolicy::best()).is_none());
    }

    #[test]
    fn pick_refuses_a_value_it_does_not_trust_enough() {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.insert(&root, BookValue::undefined());
        // 浅い中盤探索の値。窓が広い。
        book.insert(&child, BookValue::searched(-2, 40, 6, 3));
        book.propagate();

        let mut policy = PickPolicy::best();
        policy.min_trust = Trust::High;
        assert!(book.pick(&root, &policy).is_none());

        policy.min_trust = Trust::Low;
        assert!(book.pick(&root, &policy).is_some());
    }

    #[test]
    fn randomness_zero_always_picks_the_best() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        let mut legal = root.moves();
        let mut score = 0i8;
        while legal != 0 {
            let mv = legal.trailing_zeros();
            legal &= legal - 1;
            book.insert(&root.make_move(1u64 << mv), BookValue::exact(score));
            score += 1;
        }
        book.set_random_seed(7);

        let policy = PickPolicy {
            max_loss: 20,
            randomness: 0.0,
            ..PickPolicy::default()
        };
        let first = book.pick(&root, &policy).unwrap();
        for _ in 0..20 {
            assert_eq!(book.pick(&root, &policy).unwrap().mv, first.mv);
        }
    }

    #[test]
    fn randomness_one_spreads_over_the_allowed_moves() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        let mut legal = root.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros();
            legal &= legal - 1;
            book.insert(&root.make_move(1u64 << mv), BookValue::exact(0));
        }
        book.set_random_seed(7);

        let policy = PickPolicy {
            max_loss: 0,
            randomness: 1.0,
            ..PickPolicy::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            seen.insert(book.pick(&root, &policy).unwrap().mv);
        }
        assert!(seen.len() > 1, "同じ値の手が 1 つしか選ばれない");
    }

    #[test]
    fn merge_keeps_the_more_trustworthy_value() {
        let mut a = Book::empty();
        let board = Board::new();
        a.insert(&board, BookValue::searched(1, 60, 10, 3));

        let mut b = Book::empty();
        b.insert(&board, BookValue::searched(9, 60, 4, 3));
        assert_eq!(a.merge(&b), 0);
        assert_eq!(a.value_of(&board).unwrap().score, 1, "浅い方は無視される");

        let mut c = Book::empty();
        c.insert(&board, BookValue::searched(7, 60, 20, 3));
        a.merge(&c);
        assert_eq!(a.value_of(&board).unwrap().score, 7, "深い方を採る");
    }

    #[test]
    fn merge_brings_the_opening_names_along() {
        let mut a = Book::empty();
        let mut b = Book::empty();
        b.names_mut().insert("虎", "F5D6C3D3C4").unwrap();
        a.merge(&b);
        assert_eq!(a.names().len(), 1);
    }

    #[test]
    fn line_follows_the_book_until_it_runs_out() {
        let mut book = sample_book();
        let line = book.line(&Board::new(), &PickPolicy::best());
        assert_eq!(line.len(), 2);
        assert_ne!(Board::new().moves() & (1u64 << line[0]), 0);
    }

    #[test]
    fn prune_unreachable_drops_orphans() {
        let mut book = sample_book();
        // 初期盤面から 1 手では絶対に届かない局面を ply 1 に混ぜる。
        let orphan = Board {
            player: 0x0000_0018_1800_0000,
            opponent: 0x0000_0000_0000_00FF,
        };
        book.table_mut().layer_mut(1).insert(
            orphan.unique_board(),
            BookValue::exact(0),
            Frontier::unset(),
        );
        assert_eq!(book.len(), 4);

        assert_eq!(book.prune_unreachable(), 1);
        assert_eq!(book.len(), 3);
        assert!(book.contains(&Board::new().make_move(1u64 << D3)));
    }

    #[test]
    fn reduce_keeps_children_whose_value_is_not_known_yet() {
        let mut book = Book::empty();
        let root = Board::new().make_move(1u64 << D3);
        book.insert(&Board::new(), BookValue::exact(0));
        book.insert(&root, BookValue::exact(0));
        let mut legal = root.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros();
            legal &= legal - 1;
            book.insert(&root.make_move(1u64 << mv), BookValue::undefined());
        }
        let before = book.len();
        assert_eq!(book.reduce(0), 0, "値が分からない子は消さない");
        assert_eq!(book.len(), before);
    }

    #[test]
    fn reduce_drops_lines_that_lose_too_much() {
        let mut book = Book::empty();
        // D3 の後は 3 通りの別々の正規形に分かれる。
        let root = Board::new().make_move(1u64 << D3);
        book.insert(&Board::new(), BookValue::exact(-4));
        book.insert(&root, BookValue::exact(4));

        let mut legal = root.moves();
        let mut score = 0i8;
        while legal != 0 {
            let mv = legal.trailing_zeros();
            legal &= legal - 1;
            // 0, -4, -8 と離す。
            book.insert(&root.make_move(1u64 << mv), BookValue::exact(score));
            score -= 4;
        }
        let before = book.len();
        assert!(before > 3);

        // 4 石以上損する変化を削る。
        let removed = book.reduce(4);
        assert!(removed > 0, "何も削られていない");
        // 最善 (子の値 0 = 親から見て 0) から 4 以内の子だけが残る。
        for (_, board, _) in book.iter() {
            assert!(book.contains(board));
        }
    }
}
