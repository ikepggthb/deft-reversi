use deft_reversi_engine::{Action, Board, MoveError, Square, perft};

// Deliberately uses coordinates rather than the engine's bit operations.
fn reference_flips(board: Board, index: i32) -> u64 {
    let bit = 1u64 << index;
    if (board.player_bits() | board.opponent_bits()) & bit != 0 {
        return 0;
    }
    let mut flips = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (mut x, mut y) = (index % 8 + dx, index / 8 + dy);
            let mut ray = 0;
            while (0..8).contains(&x) && (0..8).contains(&y) {
                let next = 1u64 << (y * 8 + x);
                if board.opponent_bits() & next != 0 {
                    ray |= next;
                } else {
                    if board.player_bits() & next != 0 {
                        flips |= ray;
                    }
                    break;
                }
                x += dx;
                y += dy;
            }
        }
    }
    flips
}

#[test]
fn reachable_positions_match_coordinate_reference() {
    let mut random = 0x1234_5678_9abc_def0u64;
    for _ in 0..128 {
        let mut board = Board::new();
        loop {
            let mut expected = 0;
            for index in 0..64 {
                let flips = reference_flips(board, index);
                if flips != 0 {
                    expected |= 1u64 << index;
                    assert_eq!(board.flips(Square::new(index as u8).unwrap()), flips);
                }
            }
            assert_eq!(board.legal_moves().bits(), expected);
            if expected == 0 {
                if board.is_terminal() {
                    break;
                }
                board = board.play(Action::Pass).unwrap();
                continue;
            }
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut moves = expected;
            for _ in 0..random % u64::from(moves.count_ones()) {
                moves &= moves - 1;
            }
            let action = moves.isolate_lowest_one();
            let flips = reference_flips(board, action.trailing_zeros() as i32);
            let expected_board = Board::from_bits(
                board.opponent_bits() & !flips,
                board.player_bits() | flips | action,
            )
            .unwrap();
            let square = Square::new(action.trailing_zeros() as u8).unwrap();
            board = board.play(Action::Place(square)).unwrap();
            assert_eq!(board, expected_board);
        }
    }
}

#[test]
fn rejected_actions_are_errors() {
    let board = Board::new();
    for index in [0, 27, 28, 63] {
        let action = Action::Place(Square::new(index).unwrap());
        assert_eq!(board.play(action), Err(MoveError::IllegalMove));
        assert_eq!(board.flips(Square::new(index).unwrap()), 0);
    }
    assert_eq!(board.play(Action::Pass), Err(MoveError::IllegalPass));
    assert!(Board::from_bits(1, 1).is_none());
    let terminal = Board::from_bits(u64::MAX, 0).unwrap();
    assert!(terminal.is_terminal());
    assert!(!terminal.must_pass());
    assert_eq!(terminal.play(Action::Pass), Err(MoveError::IllegalPass));
}

#[test]
fn initial_position_perft() {
    for (depth, expected) in [1, 4, 12, 56, 244, 1396, 8200, 55092, 390216]
        .into_iter()
        .enumerate()
    {
        assert_eq!(perft(Board::new(), depth as u8), expected, "depth {depth}");
    }
}
