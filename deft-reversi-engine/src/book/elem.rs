//! book に格納する 1 局面と、Egaroucid の対称変換。
//!
//! Egaroucid の `Book_elem` / `Leaf` (src/engine/book.hpp) と
//! `representative_board` / `convert_coord_*` (src/engine/util.hpp) に対応する。
//!
//! # 座標系について
//!
//! Egaroucid は A1 をビット 63 に置き、このエンジンと Edax は A1 をビット 0 に
//! 置く。この違いはビット列の 180 度回転にあたり、180 度回転は対称変換の群
//! (D4, 位数 8) の元なので、**ある局面の 8 対称形が作る `(player, opponent)` の
//! 集合は両者で完全に一致する**。正規形はその集合の辞書順最小なので、
//! Egaroucid が保存する盤面のビット列はこのエンジンの [`Board::unique_board`]
//! が返すビット列とそのまま一致する。
//!
//! 着手も「u64 のどのビットか」を指すので変換不要でやり取りできる。人間向けの
//! 表示名 (A1/H8) だけが 180 度ずれるが、盤面も同じだけずれているので
//! 一貫している。この性質は本モジュールの
//! `representative_is_invariant_under_the_egaroucid_bit_order` で検証している。

use crate::board::board::Board;

/// パスを表す疑似座標。Egaroucid の `MOVE_PASS` (Edax の `PASS` と同値)。
pub const MOVE_PASS: i8 = 64;
/// 「もう展開する手が無い」ことを表す疑似座標。Egaroucid の `MOVE_NOMOVE`。
pub const MOVE_NOMOVE: i8 = 65;
/// 「未設定」を表す疑似座標。Egaroucid の `MOVE_UNDEFINED`。
pub const MOVE_UNDEFINED: i8 = 125;
/// 「値が無い」ことを表すスコア。Egaroucid の `SCORE_UNDEFINED`。
pub const SCORE_UNDEFINED: i8 = -126;
/// 「レベル未設定」。Egaroucid の `LEVEL_UNDEFINED`。
pub const LEVEL_UNDEFINED: i8 = -1;
/// 石差スコアの絶対値の上限。Egaroucid の `SCORE_MAX`。
pub const SCORE_MAX: i8 = 64;
/// `n_lines` の上限。Egaroucid の `MAX_N_LINES`。
pub const MAX_N_LINES: u32 = 4_000_000_000;

/// 有効な着手座標か。Egaroucid の `is_valid_policy`。
pub fn is_valid_policy(policy: i8) -> bool {
    (0..64).contains(&policy)
}

/// 有効なスコアか。Egaroucid の `is_valid_score`。
pub fn is_valid_score(score: i8) -> bool {
    (-SCORE_MAX..=SCORE_MAX).contains(&score)
}

/// book に登録されていない手のうち最善のもの。Egaroucid の `Leaf`。
///
/// Egaroucid は link を保存しないので、leaf は「まだ book に子局面が無い手」の
/// 中での最善手を意味する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub value: i8,
    /// 正規形の盤面を基準にした座標。
    pub mv: i8,
    /// この leaf を求めたときの探索レベル。
    pub level: i8,
}

impl Default for Leaf {
    fn default() -> Self {
        Self {
            value: SCORE_UNDEFINED,
            mv: MOVE_UNDEFINED,
            level: LEVEL_UNDEFINED,
        }
    }
}

impl Leaf {
    /// 実際に指せる手を指しているか。
    pub fn is_move(&self) -> bool {
        is_valid_policy(self.mv)
    }

    /// 値と手の両方が有効か。
    pub fn is_valid(&self) -> bool {
        is_valid_score(self.value) && self.is_move()
    }
}

/// book の 1 局面。Egaroucid の `Book_elem`。
///
/// Edax と違い着手リスト (link) を持たない。手はその都度、子局面が book に
/// 登録されているかを引いて求める ([`Book::moves_with_value`](super::Book::moves_with_value))。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookElem {
    /// 手番側から見た評価値(石差)。
    pub value: i8,
    /// この局面を評価したときの探索レベル。
    pub level: i8,
    pub leaf: Leaf,
    /// この局面以下の部分木のノード数。Egaroucid の `n_lines`。
    pub n_lines: u32,
}

impl Default for BookElem {
    fn default() -> Self {
        Self {
            value: SCORE_UNDEFINED,
            level: LEVEL_UNDEFINED,
            leaf: Leaf::default(),
            n_lines: 0,
        }
    }
}

impl BookElem {
    pub fn new(value: i8, level: i8) -> Self {
        Self {
            value,
            level,
            ..Self::default()
        }
    }

    /// 値が登録されているか。
    pub fn has_value(&self) -> bool {
        self.value != SCORE_UNDEFINED
    }
}

/// 問い合わせた盤面の向きに変換済みの着手。Egaroucid の `Book_value`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookMove {
    /// 問い合わせた盤面での座標 (0..64)。
    pub mv: u8,
    /// 手番側から見た評価値。
    pub value: i8,
}

/// 8 通りの対称形のうち辞書順最小の盤面と、Egaroucid の変換インデックス
/// (0..8) を返す。
///
/// Egaroucid の `representative_board(Board, int *idx)`。インデックスの意味は
/// [`convert_coord_from_representative`] を参照。
pub fn representative_board(board: &Board) -> (Board, usize) {
    // Egaroucid の探索順と一致させる。
    // 0: そのまま, 2: black line, 1: vertical, 3: black line + vertical,
    // 6: horizontal, 4: black line + horizontal, 7: h + v, 5: white line
    let b = *board;
    let bt = black_line_mirror(&b);
    let bh = horizontal_mirror(&b);
    let bth = horizontal_mirror(&bt);
    let candidates = [
        (b, 0usize),
        (bt, 2),
        (vertical_mirror(&b), 1),
        (vertical_mirror(&bt), 3),
        (bh, 6),
        (bth, 4),
        (vertical_mirror(&bh), 7),
        (vertical_mirror(&bth), 5),
    ];

    let mut best = candidates[0];
    for &cand in &candidates[1..] {
        if cand.0 < best.0 {
            best = cand;
        }
    }
    best
}

/// 正規形の盤面での座標を、元の盤面の向きに戻す。
///
/// Egaroucid の `convert_coord_from_representative_board`。
pub fn convert_coord_from_representative(cell: u8, idx: usize) -> u8 {
    let y = (cell / 8) as i32;
    let x = (cell % 8) as i32;
    let res = match idx {
        0 => cell as i32,
        1 => (7 - y) * 8 + x,       // vertical
        2 => (7 - x) * 8 + (7 - y), // black line
        3 => (7 - x) * 8 + y,       // black line + vertical (時計回り 90 度)
        4 => x * 8 + (7 - y),       // black line + horizontal (反時計回り 90 度)
        5 => x * 8 + y,             // white line
        6 => y * 8 + (7 - x),       // horizontal
        7 => (7 - y) * 8 + (7 - x), // 180 度回転
        _ => return MOVE_UNDEFINED as u8,
    };
    res as u8
}

/// 元の盤面での座標を、正規形の盤面の向きに移す。
///
/// Egaroucid の `convert_coord_to_representative_board`。idx 3 と 4 だけが
/// [`convert_coord_from_representative`] と入れ替わる(残りは自己逆変換)。
pub fn convert_coord_to_representative(cell: u8, idx: usize) -> u8 {
    let y = (cell / 8) as i32;
    let x = (cell % 8) as i32;
    let res = match idx {
        0 => cell as i32,
        1 => (7 - y) * 8 + x,
        2 => (7 - x) * 8 + (7 - y),
        3 => x * 8 + (7 - y),
        4 => (7 - x) * 8 + y,
        5 => x * 8 + y,
        6 => y * 8 + (7 - x),
        7 => (7 - y) * 8 + (7 - x),
        _ => return MOVE_UNDEFINED as u8,
    };
    res as u8
}

/// `mv` を着手した後の盤面。非合法手や疑似座標では `None`。
pub fn next_board(board: &Board, mv: i8) -> Option<Board> {
    if mv == MOVE_PASS {
        if board.moves() != 0 {
            return None;
        }
        return Some(board.passed());
    }
    if !is_valid_policy(mv) {
        return None;
    }
    let move_bit = 1u64 << mv;
    if board.moves() & move_bit == 0 {
        return None;
    }
    Some(board.make_move(move_bit))
}

/// 探索スコアを book の 1 バイトスコアに丸める。
pub fn clamp_score(score: i32) -> i8 {
    score.clamp(-(SCORE_MAX as i32), SCORE_MAX as i32) as i8
}

// ---- 盤面の対称変換 (Egaroucid の Board::board_*_mirror) ----

fn vertical_mirror(board: &Board) -> Board {
    Board {
        player: bit_vertical_mirror(board.player),
        opponent: bit_vertical_mirror(board.opponent),
    }
}

fn horizontal_mirror(board: &Board) -> Board {
    Board {
        player: bit_horizontal_mirror(board.player),
        opponent: bit_horizontal_mirror(board.opponent),
    }
}

fn black_line_mirror(board: &Board) -> Board {
    Board {
        player: bit_black_line_mirror(board.player),
        opponent: bit_black_line_mirror(board.opponent),
    }
}

/// 行を反転する。Egaroucid の `vertical_mirror`。
fn bit_vertical_mirror(x: u64) -> u64 {
    x.swap_bytes()
}

/// 各行のビットを反転する。Egaroucid の `horizontal_mirror`。
fn bit_horizontal_mirror(x: u64) -> u64 {
    let x = ((x >> 1) & 0x5555_5555_5555_5555) | ((x << 1) & 0xAAAA_AAAA_AAAA_AAAA);
    let x = ((x >> 2) & 0x3333_3333_3333_3333) | ((x << 2) & 0xCCCC_CCCC_CCCC_CCCC);
    ((x >> 4) & 0x0F0F_0F0F_0F0F_0F0F) | ((x << 4) & 0xF0F0_F0F0_F0F0_F0F0)
}

/// 反対角線で折り返す。Egaroucid の `black_line_mirror`。
fn bit_black_line_mirror(x: u64) -> u64 {
    let a = (x ^ (x >> 9)) & 0x0055_0055_0055_0055;
    let x = x ^ a ^ (a << 9);
    let a = (x ^ (x >> 18)) & 0x0000_3333_0000_3333;
    let x = x ^ a ^ (a << 18);
    let a = (x ^ (x >> 36)) & 0x0000_0000_0F0F_0F0F;
    x ^ a ^ (a << 36)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u8 = 19;

    fn played_board() -> Board {
        let board = Board::new().make_move(1u64 << D3);
        board.make_move(1u64 << board.moves().trailing_zeros())
    }

    #[test]
    fn representative_board_is_the_lexicographic_minimum() {
        let board = played_board();
        let (rep, _) = representative_board(&board);

        assert_eq!(rep, board.all_symmetries().into_iter().min().unwrap());
    }

    /// Egaroucid の正規形は、このエンジンの `unique_board` と一致する。
    /// (座標系が 180 度違うのに一致する理由は本モジュールの説明を参照)
    #[test]
    fn representative_board_matches_unique_board() {
        let mut board = Board::new();
        for _ in 0..30 {
            let (rep, _) = representative_board(&board);
            assert_eq!(rep, board.unique_board());
            let legal = board.moves();
            if legal == 0 {
                board = board.passed();
                if board.moves() == 0 {
                    break;
                }
                continue;
            }
            board = board.make_move(1u64 << legal.trailing_zeros());
        }
    }

    /// Egaroucid は A1 をビット 63 に置く (このエンジンと Edax はビット 0)。
    /// 同じ物理局面を反対のビット順で表しても正規形の u64 が一致することを
    /// 確かめる。これが「Egaroucid の盤面をそのまま読める」根拠。
    #[test]
    fn representative_is_invariant_under_the_egaroucid_bit_order() {
        let mut state = 0x2026_0807_dead_beefu64;
        let mut next = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };

        for _ in 0..20_000 {
            let a = next();
            let b = next();
            let player = a & !b;
            let opponent = b & !a;
            let ours = Board { player, opponent };
            // 180 度回転 = ビット反転が Egaroucid との座標系の差。
            let egaroucid = Board {
                player: player.reverse_bits(),
                opponent: opponent.reverse_bits(),
            };

            assert_eq!(
                representative_board(&ours).0,
                representative_board(&egaroucid).0,
                "player={player:#018x} opponent={opponent:#018x}"
            );
        }
    }

    #[test]
    fn representative_board_is_stable_over_all_symmetries() {
        let board = played_board();
        let (rep, _) = representative_board(&board);
        for sym in board.all_symmetries() {
            assert_eq!(representative_board(&sym).0, rep);
        }
    }

    #[test]
    fn coord_conversion_round_trips_for_every_symmetry() {
        for idx in 0..8 {
            for cell in 0..64u8 {
                let moved = convert_coord_from_representative(cell, idx);
                assert_eq!(convert_coord_to_representative(moved, idx), cell);
            }
        }
    }

    #[test]
    fn coord_conversion_follows_the_board_transformation() {
        // 正規形での合法手を元の向きに戻すと、元の盤面の合法手になる。
        let board = played_board();
        let (rep, idx) = representative_board(&board);

        let mut legal = rep.moves();
        assert_ne!(legal, 0);
        while legal != 0 {
            let cell = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            let original = convert_coord_from_representative(cell, idx);
            assert_ne!(
                board.moves() & (1u64 << original),
                0,
                "idx={idx} cell={cell} -> {original}"
            );
            // 着手後も対応が保たれる。
            let after_rep = rep.make_move(1u64 << cell);
            let after_original = board.make_move(1u64 << original);
            assert_eq!(
                representative_board(&after_rep).0,
                representative_board(&after_original).0
            );
        }
    }

    #[test]
    fn next_board_handles_pass_and_illegal_moves() {
        let board = Board::new();
        assert!(next_board(&board, D3 as i8).is_some());
        assert!(next_board(&board, 0).is_none());
        assert!(next_board(&board, MOVE_PASS).is_none());
        assert!(next_board(&board, MOVE_NOMOVE).is_none());
        assert!(next_board(&board, MOVE_UNDEFINED).is_none());
    }

    #[test]
    fn constants_match_egaroucid() {
        assert_eq!(MOVE_PASS, 64);
        assert_eq!(MOVE_NOMOVE, 65);
        assert_eq!(MOVE_UNDEFINED, 125);
        assert_eq!(SCORE_UNDEFINED, -126);
        assert_eq!(LEVEL_UNDEFINED, -1);
        assert!(!is_valid_policy(MOVE_PASS));
        assert!(!is_valid_score(SCORE_UNDEFINED));
        assert!(is_valid_score(-64) && is_valid_score(64));
    }
}
