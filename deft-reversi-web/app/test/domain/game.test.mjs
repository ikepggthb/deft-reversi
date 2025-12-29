import test from 'node:test';
import assert from 'node:assert/strict';
import { Game } from '../../domain/game.js';
import { Board } from '../../domain/board.js';
import { GameRecord } from '../../domain/game-record.js';
import { Turn, positionToNotation, notationToPosition } from '../../domain/types.js';

test('Game.newGame: creates initial state', () => {
    const game = Game.newGame();
    assert.ok(game.board instanceof Board);
    assert.ok(game.record instanceof GameRecord);
    assert.equal(game.record.length, 0);
    assert.equal(game.lastMove, null);
    assert.equal(game.isGameOver, false);
    assert.equal(game.nextTurn, Turn.BLACK);
});

test('Game.applyMove: updates board and record', () => {
    const game = Game.newGame();
    const newGame = game.applyMove(37); // f5

    // Original unchanged
    assert.equal(game.record.length, 0);
    assert.equal(game.lastMove, null);

    // New game updated
    assert.equal(newGame.record.length, 1);
    assert.equal(newGame.record.moves[0], 'f5');
    assert.equal(newGame.lastMove, 37);
    assert.equal(newGame.nextTurn, Turn.WHITE);
});

test('Game.applyPass: updates board and record', () => {
    const game = Game.newGame();
    const passed = game.applyPass();

    assert.equal(passed.record.length, 1);
    assert.equal(passed.record.moves[0], 'pass');
    assert.equal(passed.lastMove, null); // lastMove unchanged on pass
    assert.equal(passed.nextTurn, Turn.WHITE);
});

test('Game.canPlace: delegates to board', () => {
    const game = Game.newGame();
    assert.equal(game.canPlace(37), true); // f5 is legal
    assert.equal(game.canPlace(0), false); // a1 is not legal
});

test('Game.legalMovePositions: delegates to board', () => {
    const game = Game.newGame();
    const moves = game.legalMovePositions;
    assert.deepEqual(moves.sort((a, b) => a - b), [19, 26, 37, 44]);
});

test('Game.snapshot and fromSnapshot: roundtrip', () => {
    let game = Game.newGame();
    game = game.applyMove(37); // f5
    game = game.applyMove(43); // d6

    const snapshot = game.snapshot();

    // Verify snapshot structure
    assert.equal(typeof snapshot.blackBits, 'bigint');
    assert.equal(typeof snapshot.whiteBits, 'bigint');
    assert.equal(snapshot.nextTurn, Turn.BLACK);
    assert.deepEqual(snapshot.recordMoves, ['f5', 'd6']);
    assert.equal(snapshot.lastMove, 43);

    // Restore from snapshot
    const restored = Game.fromSnapshot(snapshot);
    assert.equal(restored.board.blackBits, game.board.blackBits);
    assert.equal(restored.board.whiteBits, game.board.whiteBits);
    assert.equal(restored.nextTurn, game.nextTurn);
    assert.equal(restored.record.length, game.record.length);
    assert.equal(restored.lastMove, game.lastMove);
});

test('Game.getResult: returns correct counts', () => {
    const game = Game.newGame();
    const result = game.getResult();

    assert.equal(result.blackCount, 2);
    assert.equal(result.whiteCount, 2);
    assert.equal(result.winner, null); // Tie at start
});

test('Game.toKey: returns unique key', () => {
    const game1 = Game.newGame();
    const game2 = Game.newGame();
    const game3 = game1.applyMove(37);

    assert.equal(game1.toKey(), game2.toKey());
    assert.notEqual(game1.toKey(), game3.toKey());
});

test('Game is frozen (immutable)', () => {
    const game = Game.newGame();
    assert.ok(Object.isFrozen(game));
});

// GameRecord tests
test('GameRecord: empty record', () => {
    const record = new GameRecord();
    assert.equal(record.length, 0);
    assert.deepEqual(record.moves, []);
    assert.equal(record.toString(), '');
});

test('GameRecord.addMove: returns new record with move', () => {
    const record = new GameRecord();
    const newRecord = record.addMove(37); // f5

    assert.equal(record.length, 0); // Original unchanged
    assert.equal(newRecord.length, 1);
    assert.equal(newRecord.moves[0], 'f5');
});

test('GameRecord.addPass: returns new record with pass', () => {
    const record = new GameRecord();
    const newRecord = record.addPass();

    assert.equal(newRecord.length, 1);
    assert.equal(newRecord.moves[0], 'pass');
});

test('GameRecord.toString: space-separated moves', () => {
    let record = new GameRecord();
    record = record.addMove(37); // f5
    record = record.addMove(43); // d6
    record = record.addPass();
    record = record.addMove(18); // c3

    assert.equal(record.toString(), 'f5 d6 pass c3');
});

test('GameRecord is frozen (immutable)', () => {
    const record = new GameRecord(['f5', 'd6']);
    assert.ok(Object.isFrozen(record));
    assert.ok(Object.isFrozen(record.moves));
});

test('GameRecord.equals: compares records correctly', () => {
    const record1 = new GameRecord(['f5', 'd6']);
    const record2 = new GameRecord(['f5', 'd6']);
    const record3 = new GameRecord(['f5']);

    assert.equal(record1.equals(record2), true);
    assert.equal(record1.equals(record3), false);
});

// Types tests
test('positionToNotation: converts position to notation', () => {
    assert.equal(positionToNotation(0), 'a1');
    assert.equal(positionToNotation(7), 'h1');
    assert.equal(positionToNotation(8), 'a2');
    assert.equal(positionToNotation(37), 'f5');
    assert.equal(positionToNotation(63), 'h8');
});

test('notationToPosition: converts notation to position', () => {
    assert.equal(notationToPosition('a1'), 0);
    assert.equal(notationToPosition('h1'), 7);
    assert.equal(notationToPosition('a2'), 8);
    assert.equal(notationToPosition('f5'), 37);
    assert.equal(notationToPosition('h8'), 63);
    assert.equal(notationToPosition('invalid'), null);
    assert.equal(notationToPosition(''), null);
});

test('positionToNotation and notationToPosition: roundtrip', () => {
    for (let pos = 0; pos < 64; pos++) {
        const notation = positionToNotation(pos);
        const back = notationToPosition(notation);
        assert.equal(back, pos);
    }
});
