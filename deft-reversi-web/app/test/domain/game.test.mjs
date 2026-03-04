import test from 'node:test';
import assert from 'node:assert/strict';
import { Game } from '../../domain/game.js';
import { Board } from '../../domain/board.js';
import { Turn, positionToNotation, notationToPosition } from '../../domain/types.js';

test('Game.newGame: creates initial state', () => {
    const game = Game.newGame();
    assert.ok(game.board instanceof Board);
    assert.ok(Array.isArray(game.record));
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
    assert.equal(newGame.record[0], 'f5');
    assert.equal(newGame.lastMove, 37);
    assert.equal(newGame.nextTurn, Turn.WHITE);
});

test('Game.applyPass: updates board and record', () => {
    const game = Game.newGame();
    const passed = game.applyPass();

    assert.equal(passed.record.length, 1);
    assert.equal(passed.record[0], 'pass');
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
