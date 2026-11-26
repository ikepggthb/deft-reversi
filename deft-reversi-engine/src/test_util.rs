//! ユニットテスト間で共有する、再現性のあるランダム局面生成ユーティリティです。
use crate::Board;

/// xorshift64 擬似乱数生成器を 1 ステップ進め、新しい乱数値を返します。
pub(crate) fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// 空でない合法手マスクから 1 手をランダムに選び、そのマスだけを立てたビットマスクを返します。
pub(crate) fn random_move(mut moves: u64, random: &mut u64) -> u64 {
    for _ in 0..xorshift(random) % u64::from(moves.count_ones()) {
        moves &= moves - 1;
    }
    moves.isolate_lowest_one()
}

/// 初期局面から必要に応じてパスを挟みながらランダムに着手を進め、
/// 空きマス数が `empties` 以下になるか終局に達した時点の盤面を返します。
pub(crate) fn random_board(seed: u64, empties: u8) -> Board {
    let mut board = Board::new();
    let mut random = seed;
    while board.empty_count() > empties && !board.is_terminal() {
        let moves = board.get_moves();
        if moves == 0 {
            board = board.board_after_pass();
            continue;
        }
        board = board.board_after_move(random_move(moves, &mut random));
    }
    board
}
