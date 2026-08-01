import { describe, it, mock } from 'node:test';
import assert from 'node:assert';
import { PassHandler } from '../../application/pass-handler.js';
import { Game } from '../../domain/game.js';

function createMockCallbacks() {
    return {
        onPassAnimation: mock.fn(async () => {}),
        onSnapshot: mock.fn((state) => ({ game: state.game, hintScores: null })),
        onRender: mock.fn(() => {}),
    };
}

function createSessionState(game = null) {
    return {
        game,
        history: [],
        future: [],
        hintScores: null,
        pendingAutoStart: false,
    };
}

function createMockGame(mustPass, isGameOver = false) {
    const game = Game.newGame();
    return {
        mustPass,
        isGameOver,
        applyPass() {
            // Return a new mock game that represents the state after pass
            return createMockGame(false, isGameOver);
        },
    };
}

function createNormalGame() {
    return Game.newGame();
}

describe('PassHandler', () => {
    describe('handleForcedPasses', () => {
        it('returns false when no pass is needed', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const state = createSessionState(createNormalGame());

            const result = await handler.handleForcedPasses(state);

            assert.strictEqual(result, false);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 0);
            assert.strictEqual(callbacks.onSnapshot.mock.calls.length, 0);
            assert.strictEqual(callbacks.onRender.mock.calls.length, 0);
        });

        it('handles forced pass when mustPass is true', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const mockGame = createMockGame(true, false);
            const state = createSessionState(mockGame);

            const result = await handler.handleForcedPasses(state);

            assert.strictEqual(result, true);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length >= 1, true);
            assert.strictEqual(callbacks.onSnapshot.mock.calls.length >= 1, true);
            assert.strictEqual(callbacks.onRender.mock.calls.length >= 1, true);
            assert.strictEqual(state.history.length >= 1, true);
            assert.deepStrictEqual(state.future, []);
        });

        it('returns false when game is null', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const state = createSessionState(null);

            const result = await handler.handleForcedPasses(state);

            assert.strictEqual(result, false);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 0);
        });
    });

    describe('handlePostMovePass', () => {
        it('returns false when no pass is needed', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const state = createSessionState(createNormalGame());

            const result = await handler.handlePostMovePass(state);

            assert.strictEqual(result, false);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 0);
            assert.strictEqual(callbacks.onRender.mock.calls.length, 0);
        });

        it('handles pass when mustPass is true', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const mockGame = createMockGame(true, false);
            const state = createSessionState(mockGame);
            const originalGame = state.game;

            const result = await handler.handlePostMovePass(state);

            assert.strictEqual(result, true);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 1);
            assert.strictEqual(callbacks.onRender.mock.calls.length, 1);
            assert.notStrictEqual(state.game, originalGame);
        });

        it('returns false when game is null', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const state = createSessionState(null);

            const result = await handler.handlePostMovePass(state);

            assert.strictEqual(result, false);
        });
    });

    describe('handlePostMovePassSimple', () => {
        it('returns unchanged game when no pass is needed', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const game = createNormalGame();

            const result = await handler.handlePostMovePassSimple(game);

            assert.strictEqual(result.passed, false);
            assert.strictEqual(result.game, game);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 0);
        });

        it('applies pass when mustPass is true', async () => {
            const callbacks = createMockCallbacks();
            const handler = new PassHandler(callbacks);
            const mockGame = createMockGame(true, false);

            const result = await handler.handlePostMovePassSimple(mockGame);

            assert.strictEqual(result.passed, true);
            assert.notStrictEqual(result.game, mockGame);
            assert.strictEqual(callbacks.onPassAnimation.mock.calls.length, 1);
            assert.strictEqual(callbacks.onRender.mock.calls.length, 1);
        });
    });
});
