//! 着手生成や盤面更新が正しく動作するかを検証するための診断ユーティリティです。
//!
//! 棋譜ソフトや探索エンジンのテストで広く使われる `perft`（Performance Test の略）を提供します。
//! 指定した手数先までの合法手をすべて展開し、到達する末端局面数を数え上げることで、
//! 既知の正しいノード数と一致するかを確かめられます。

use crate::Board;

/// 探索時と同じ、正当性チェックを省いた高速な盤面遷移を用いて、指定深さまでの末端局面数（葉ノード数）を数え上げます。
///
/// 通常の探索深さとは異なり、ここでの `depth` には強制パス（Pass）も 1 手（1 プライ）として数えます。
/// 両者とも合法手がなく終局した局面に到達した場合は、その時点で 1 つの末端としてカウントします。
pub fn perft(board: Board, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = board.get_moves();
    if moves == 0 {
        return if board.is_terminal() {
            1
        } else {
            perft(board.board_after_pass(), depth - 1)
        };
    }
    let mut leaves = 0;
    while moves != 0 {
        let bit = moves.isolate_lowest_one();
        moves &= moves - 1;
        leaves += perft(board.board_after_move(bit), depth - 1);
    }
    leaves
}
