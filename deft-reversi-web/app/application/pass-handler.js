/**
 * @fileoverview PassHandler - パス処理を担当するモジュール
 */

/** @typedef {import('./session-state-manager.js').SessionState} SessionState */

/**
 * パス処理のコールバック
 * @typedef {Object} PassCallbacks
 * @property {() => Promise<void>} onPassAnimation パスアニメーション表示
 * @property {(state: SessionState) => import('./session-state-manager.js').HistorySnapshot} onSnapshot スナップショット取得
 * @property {() => void} onRender 画面描画
 */

/**
 * パス処理を担当するクラス
 */
export class PassHandler {
    /**
     * @param {PassCallbacks} callbacks
     */
    constructor(callbacks) {
        this._callbacks = callbacks;
    }

    /**
     * 強制パスを処理する（ループ）
     * 自分のターンで打つ手がない場合に、連続でパスを処理する
     * @param {SessionState} state
     * @returns {Promise<boolean>} パスが発生したかどうか
     */
    async handleForcedPasses(state) {
        let passed = false;
        while (state.game?.mustPass) {
            await this._callbacks.onPassAnimation();
            state.history.push(this._callbacks.onSnapshot(state));
            state.future = [];
            state.game = state.game.applyPass();
            this._callbacks.onRender();
            passed = true;
            if (state.game.isGameOver) break;
        }
        return passed;
    }

    /**
     * 着手後のパス処理
     * 相手のターンで打つ手がない場合にパスを適用する
     * @param {SessionState} state
     * @returns {Promise<boolean>} パスが発生したかどうか
     */
    async handlePostMovePass(state) {
        if (!state.game?.mustPass) {
            return false;
        }
        await this._callbacks.onPassAnimation();
        state.game = state.game.applyPass();
        this._callbacks.onRender();
        return true;
    }

    /**
     * 着手後のパス処理（履歴を残さない、playAiMoveOnce用）
     * @param {import('../domain/game.js').Game} game
     * @returns {Promise<{ game: import('../domain/game.js').Game, passed: boolean }>}
     */
    async handlePostMovePassSimple(game) {
        if (!game.mustPass) {
            return { game, passed: false };
        }
        await this._callbacks.onPassAnimation();
        const newGame = game.applyPass();
        this._callbacks.onRender();
        return { game: newGame, passed: true };
    }
}
