import test from 'node:test';
import assert from 'node:assert/strict';
import { Board } from '../../domain/board.js';
import { Turn } from '../../domain/types.js';

const INITIAL_BLACK = 0x0000000810000000n;
const INITIAL_WHITE = 0x0000001008000000n;

test('Board.initial: creates correct starting position', () => {
    const board = Board.initial();
    assert.equal(board.blackBits, INITIAL_BLACK);
    assert.equal(board.whiteBits, INITIAL_WHITE);
    assert.equal(board.nextTurn, Turn.BLACK);
    assert.equal(board.blackCount, 2);
    assert.equal(board.whiteCount, 2);
    assert.equal(board.emptyCount, 60);
});

test('Board.fromBits: creates board from bits', () => {
    const board = Board.fromBits(INITIAL_BLACK, INITIAL_WHITE, Turn.WHITE);
    assert.equal(board.blackBits, INITIAL_BLACK);
    assert.equal(board.whiteBits, INITIAL_WHITE);
    assert.equal(board.nextTurn, Turn.WHITE);
});

test('Board.legalMovePositions: returns correct moves for initial position', () => {
    const board = Board.initial();
    const moves = board.legalMovePositions;
    // d3=19, c4=26, f5=37, e6=44
    assert.deepEqual(moves.sort((a, b) => a - b), [19, 26, 37, 44]);
});

test('Board.canPlace: returns true for legal moves', () => {
    const board = Board.initial();
    // Legal moves for black at initial position
    assert.equal(board.canPlace(19), true); // d3
    assert.equal(board.canPlace(26), true); // c4
    assert.equal(board.canPlace(37), true); // f5
    assert.equal(board.canPlace(44), true); // e6
    // Illegal moves
    assert.equal(board.canPlace(0), false);
    assert.equal(board.canPlace(27), false); // d4 (occupied by black)
    assert.equal(board.canPlace(28), false); // e4 (occupied by white)
});

test('Board.applyMove: returns new immutable board', () => {
    const board = Board.initial();
    const newBoard = board.applyMove(37); // f5

    // Original board unchanged
    assert.equal(board.blackCount, 2);
    assert.equal(board.whiteCount, 2);
    assert.equal(board.nextTurn, Turn.BLACK);

    // New board updated
    assert.equal(newBoard.blackCount, 4); // 2 + 1 placed + 1 flipped
    assert.equal(newBoard.whiteCount, 1); // 2 - 1 flipped
    assert.equal(newBoard.nextTurn, Turn.WHITE);
});

test('Board.applyMove: throws on illegal move', () => {
    const board = Board.initial();
    assert.throws(() => board.applyMove(0), /illegal move/);
});

test('Board.applyPass: switches turn without changing pieces', () => {
    const board = Board.initial();
    const passed = board.applyPass();

    assert.equal(passed.blackBits, board.blackBits);
    assert.equal(passed.whiteBits, board.whiteBits);
    assert.equal(passed.nextTurn, Turn.WHITE);
});

test('Board.playerBits/opponentBits: returns correct bits based on turn', () => {
    const boardBlack = Board.initial();
    assert.equal(boardBlack.playerBits, INITIAL_BLACK);
    assert.equal(boardBlack.opponentBits, INITIAL_WHITE);

    const boardWhite = Board.fromBits(INITIAL_BLACK, INITIAL_WHITE, Turn.WHITE);
    assert.equal(boardWhite.playerBits, INITIAL_WHITE);
    assert.equal(boardWhite.opponentBits, INITIAL_BLACK);
});

test('Board.mustPass: returns false when moves available', () => {
    const board = Board.initial();
    assert.equal(board.mustPass(), false);
});

test('Board.isGameOver: returns false for initial position', () => {
    const board = Board.initial();
    assert.equal(board.isGameOver(), false);
});

test('Board.toKey: returns unique string for state', () => {
    const board1 = Board.initial();
    const board2 = Board.initial();
    const board3 = board1.applyMove(37);

    assert.equal(board1.toKey(), board2.toKey());
    assert.notEqual(board1.toKey(), board3.toKey());
});

test('Board is frozen (immutable)', () => {
    const board = Board.initial();
    assert.ok(Object.isFrozen(board));
});

test('Board.toString: returns readable representation', () => {
    const board = Board.initial();
    const str = board.toString();
    assert.ok(str.includes('a b c d e f g h'));
    assert.ok(str.includes('Next: Black'));
});

test('Board game sequence: play multiple moves', () => {
    let board = Board.initial();

    // f5 (black)
    board = board.applyMove(37);
    assert.equal(board.nextTurn, Turn.WHITE);
    assert.equal(board.blackCount, 4);
    assert.equal(board.whiteCount, 1);

    // d6 (white)
    board = board.applyMove(43);
    assert.equal(board.nextTurn, Turn.BLACK);

    // c3 (black)
    board = board.applyMove(18);
    assert.equal(board.nextTurn, Turn.WHITE);
});
