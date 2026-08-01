import { describe, it } from 'node:test';
import assert from 'node:assert';
import { SessionStateManager } from '../../application/session-state-manager.js';
import { Game } from '../../domain/game.js';

describe('SessionStateManager', () => {
    describe('_createState', () => {
        it('creates a new session state with default values', () => {
            const manager = new SessionStateManager();
            const state = manager.getActiveState();

            assert.strictEqual(state.game, null);
            assert.deepStrictEqual(state.history, []);
            assert.deepStrictEqual(state.future, []);
            assert.strictEqual(state.hintScores, null);
            assert.strictEqual(state.pendingAutoStart, false);
        });
    });

    describe('cloneState', () => {
        it('creates a deep copy of session state', () => {
            const manager = new SessionStateManager();
            const original = manager.getActiveState();
            original.game = Game.newGame();
            original.history = [{ game: Game.newGame(), hintScores: null }];
            original.future = [{ game: Game.newGame(), hintScores: [1, 2, 3] }];
            original.hintScores = [4, 5, 6];
            original.pendingAutoStart = true;

            const cloned = manager.cloneState(original);

            assert.notStrictEqual(cloned, original);
            assert.strictEqual(cloned.game, original.game);
            assert.notStrictEqual(cloned.history, original.history);
            assert.deepStrictEqual(cloned.history, original.history);
            assert.notStrictEqual(cloned.future, original.future);
            assert.deepStrictEqual(cloned.future, original.future);
            assert.notStrictEqual(cloned.hintScores, original.hintScores);
            assert.deepStrictEqual(cloned.hintScores, original.hintScores);
            assert.strictEqual(cloned.pendingAutoStart, true);
        });

        it('handles null hintScores', () => {
            const manager = new SessionStateManager();
            const original = manager.getActiveState();
            original.hintScores = null;

            const cloned = manager.cloneState(original);

            assert.strictEqual(cloned.hintScores, null);
        });
    });

    describe('getActiveState', () => {
        it('returns main state when study mode is disabled', () => {
            const manager = new SessionStateManager();
            const mainState = manager.getMainState();
            const activeState = manager.getActiveState();

            assert.strictEqual(activeState, mainState);
        });

        it('returns study state when study mode is enabled', () => {
            const manager = new SessionStateManager();
            manager.getMainState().game = Game.newGame();
            manager.enableStudyMode(['f5', 'd6']);

            const activeState = manager.getActiveState();

            assert.notStrictEqual(activeState, manager.getMainState());
            assert.strictEqual(activeState, manager.getStudyState());
        });
    });

    describe('enableStudyMode', () => {
        it('creates study state from main state', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();
            manager.getMainState().game = game;
            manager.getMainState().hintScores = [1, 2, 3];

            manager.enableStudyMode(['f5', 'd6']);

            assert.strictEqual(manager.isStudyModeEnabled, true);
            assert.notStrictEqual(manager.getStudyState(), manager.getMainState());
            assert.strictEqual(manager.getStudyState()?.game, game);
            assert.strictEqual(manager.getStudyState()?.pendingAutoStart, false);
            assert.deepStrictEqual(manager.studyBranchStartRecord, ['f5', 'd6']);
        });

        it('does nothing if main game is null', () => {
            const manager = new SessionStateManager();

            manager.enableStudyMode(['f5', 'd6']);

            assert.strictEqual(manager.isStudyModeEnabled, false);
            assert.strictEqual(manager.getStudyState(), null);
        });

        it('does nothing if already in study mode', () => {
            const manager = new SessionStateManager();
            manager.getMainState().game = Game.newGame();
            manager.enableStudyMode(['f5']);

            const studyState = manager.getStudyState();
            manager.enableStudyMode(['d6', 'c3']);

            assert.strictEqual(manager.getStudyState(), studyState);
            assert.deepStrictEqual(manager.studyBranchStartRecord, ['f5']);
        });
    });

    describe('disableStudyMode', () => {
        it('clears study state and returns to main state', () => {
            const manager = new SessionStateManager();
            manager.getMainState().game = Game.newGame();
            manager.enableStudyMode(['f5', 'd6']);

            manager.disableStudyMode();

            assert.strictEqual(manager.isStudyModeEnabled, false);
            assert.strictEqual(manager.getStudyState(), null);
            assert.deepStrictEqual(manager.studyBranchStartRecord, []);
            assert.strictEqual(manager.getActiveState(), manager.getMainState());
        });
    });

    describe('game accessor', () => {
        it('gets and sets game on active state', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();

            manager.game = game;

            assert.strictEqual(manager.game, game);
            assert.strictEqual(manager.getMainState().game, game);
        });

        it('sets game on study state when in study mode', () => {
            const manager = new SessionStateManager();
            const mainGame = Game.newGame();
            manager.game = mainGame;
            manager.enableStudyMode([]);

            const studyGame = Game.newGame().applyMove(37);
            manager.game = studyGame;

            assert.strictEqual(manager.game, studyGame);
            assert.strictEqual(manager.getMainState().game, mainGame);
            assert.strictEqual(manager.getStudyState()?.game, studyGame);
        });
    });

    describe('history and future accessors', () => {
        it('gets and sets history on active state', () => {
            const manager = new SessionStateManager();
            const snapshot = { game: Game.newGame(), hintScores: null };

            manager.history = [snapshot];

            assert.deepStrictEqual(manager.history, [snapshot]);
        });

        it('gets and sets future on active state', () => {
            const manager = new SessionStateManager();
            const snapshot = { game: Game.newGame(), hintScores: [1, 2] };

            manager.future = [snapshot];

            assert.deepStrictEqual(manager.future, [snapshot]);
        });
    });

    describe('hintScores accessor', () => {
        it('gets and sets hintScores on active state', () => {
            const manager = new SessionStateManager();
            const scores = [1, 2, 3, null, 5];

            manager.hintScores = scores;

            assert.deepStrictEqual(manager.hintScores, scores);
        });
    });

    describe('pendingAutoStart accessor', () => {
        it('gets and sets pendingAutoStart on active state', () => {
            const manager = new SessionStateManager();

            manager.pendingAutoStart = true;
            assert.strictEqual(manager.pendingAutoStart, true);

            manager.pendingAutoStart = false;
            assert.strictEqual(manager.pendingAutoStart, false);
        });

        it('coerces to boolean', () => {
            const manager = new SessionStateManager();

            manager.pendingAutoStart = 1;
            assert.strictEqual(manager.pendingAutoStart, true);

            manager.pendingAutoStart = 0;
            assert.strictEqual(manager.pendingAutoStart, false);
        });
    });

    describe('snapshot', () => {
        it('creates a snapshot of the active state', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();
            manager.game = game;
            manager.hintScores = [1, 2, 3];

            const snapshot = manager.snapshot();

            assert.strictEqual(snapshot.game, game);
            assert.deepStrictEqual(snapshot.hintScores, [1, 2, 3]);
            assert.notStrictEqual(snapshot.hintScores, manager.hintScores);
        });

        it('handles null hintScores', () => {
            const manager = new SessionStateManager();
            manager.game = Game.newGame();
            manager.hintScores = null;

            const snapshot = manager.snapshot();

            assert.strictEqual(snapshot.hintScores, null);
        });
    });

    describe('snapshotState', () => {
        it('creates a snapshot of the given state', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();
            const state = {
                game,
                history: [],
                future: [],
                hintScores: [4, 5, 6],
                pendingAutoStart: false,
            };

            const snapshot = manager.snapshotState(state);

            assert.strictEqual(snapshot.game, game);
            assert.deepStrictEqual(snapshot.hintScores, [4, 5, 6]);
        });
    });

    describe('pushHistory', () => {
        it('adds a snapshot to history', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();
            manager.game = game;
            manager.hintScores = [1, 2];

            manager.pushHistory();

            assert.strictEqual(manager.history.length, 1);
            assert.strictEqual(manager.history[0].game, game);
            assert.deepStrictEqual(manager.history[0].hintScores, [1, 2]);
        });

        it('does nothing if game is null', () => {
            const manager = new SessionStateManager();

            manager.pushHistory();

            assert.strictEqual(manager.history.length, 0);
        });
    });

    describe('clearFuture', () => {
        it('clears the future array', () => {
            const manager = new SessionStateManager();
            manager.future = [
                { game: Game.newGame(), hintScores: null },
                { game: Game.newGame(), hintScores: null },
            ];

            manager.clearFuture();

            assert.deepStrictEqual(manager.future, []);
        });
    });

    describe('restore', () => {
        it('restores game and hintScores from snapshot', () => {
            const manager = new SessionStateManager();
            const game = Game.newGame();
            const snapshot = { game, hintScores: [7, 8, 9] };

            manager.restore(snapshot);

            assert.strictEqual(manager.game, game);
            assert.deepStrictEqual(manager.hintScores, [7, 8, 9]);
        });
    });

    describe('clearStudyState', () => {
        it('clears study mode and related state', () => {
            const manager = new SessionStateManager();
            manager.getMainState().game = Game.newGame();
            manager.enableStudyMode(['f5']);

            manager.clearStudyState();

            assert.strictEqual(manager.isStudyModeEnabled, false);
            assert.strictEqual(manager.getStudyState(), null);
            assert.deepStrictEqual(manager.studyBranchStartRecord, []);
        });
    });

    describe('resetMainState', () => {
        it('resets main state to initial values', () => {
            const manager = new SessionStateManager();
            manager.game = Game.newGame();
            manager.history = [{ game: Game.newGame(), hintScores: null }];
            manager.hintScores = [1, 2, 3];

            manager.resetMainState();

            assert.strictEqual(manager.game, null);
            assert.deepStrictEqual(manager.history, []);
            assert.deepStrictEqual(manager.future, []);
            assert.strictEqual(manager.hintScores, null);
        });
    });
});
