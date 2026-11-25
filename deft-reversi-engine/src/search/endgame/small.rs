//! 空き 3 マス以下向けのスコア専用高速ソルバです。
//! 合法手生成を行わず、空きマスを直接列挙して隣接する相手石の有無と裏返る石数で判定します。
use super::last_square_bound;
use super::parity::QUADRANT_MASKS as REGIONS;
use super::*;

/// 残り 3 マス以下の探索で引き回す局面状態です。
pub(super) struct SmallPosition {
    pub(super) board: Board,
    pub(super) empty: u64,
    pub(super) odd: u64,
    pub(super) count: u8,
}
/// 各マスの周囲 8 近傍のマスクテーブルです（隣接する相手石が 0 なら着手不可として枝刈りするために使います）。
const NEIGHBORS: [u64; 64] = neighbors();
const fn neighbors() -> [u64; 64] {
    let mut out = [0; 64];
    let mut square = 0;
    while square < 64 {
        let bit = 1u64 << square;
        let horizontal = ((bit << 1) & 0xfefefefefefefefe) | ((bit >> 1) & 0x7f7f7f7f7f7f7f7f);
        out[square] = horizontal | ((horizontal | bit) << 8) | ((horizontal | bit) >> 8);
        square += 1;
    }
    out
}
impl Context {
    /// 空き 3 マス以下の局面を奇数領域優先で探索し、厳密スコア（石差）を返します。
    pub(super) fn small_score(
        &mut self,
        position: SmallPosition,
        mut alpha: i32,
        beta: i32,
        passed: bool,
    ) -> Result<i32, Aborted> {
        let SmallPosition {
            board,
            empty,
            odd,
            count,
        } = position;
        if count <= 1 {
            self.stats.leaves += 1;
            return Ok(last_square_bound(board, empty, alpha));
        }
        let mut moves = empty;
        let mut preferred = moves & odd;
        let mut best = -65;
        let mut legal = false;
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options.isolate_lowest_one();
            moves &= !bit;
            preferred &= !bit;
            let square = bit.trailing_zeros() as usize;
            if NEIGHBORS[square] & board.opponent_bits() == 0 {
                continue;
            }
            let flips = board.flip_mask(bit);
            if flips == 0 {
                continue;
            }
            legal = true;
            if flips == board.opponent_bits() {
                self.stats.leaves += 1;
                return Ok(64);
            }
            let child = board.after_known_move(bit, flips);
            self.enter()?;
            let score = if count == 2 {
                self.stats.leaves += 1;
                -last_square_bound(child, empty ^ bit, -beta)
            } else {
                let region = (square & 4) / 4 + if square >= 32 { 2 } else { 0 };
                -self.small_score(
                    SmallPosition {
                        board: child,
                        empty: empty ^ bit,
                        odd: odd ^ REGIONS[region],
                        count: count - 1,
                    },
                    -beta,
                    -alpha,
                    false,
                )?
            };
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                return Ok(best);
            }
        }
        if legal {
            return Ok(best);
        }
        if passed {
            self.stats.leaves += 1;
            return Ok(board.final_score());
        }
        self.enter()?;
        self.small_score(
            SmallPosition {
                board: board.board_after_pass(),
                empty,
                odd,
                count,
            },
            -beta,
            -alpha,
            true,
        )
        .map(|score| -score)
    }
}
