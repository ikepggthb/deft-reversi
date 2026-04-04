/**
 * @fileoverview HintService - ヒント計算を管理するサービス
 */

import { Board } from '../domain/board.js';

/**
 * @typedef {import('../infrastructure/ai-engine.js').AiEngine} AiEngine
 */

/**
 * ヒント計算を管理するサービス
 * 反復深化による逐次評価を行い、各手の評価値をコールバックで通知する
 */
export class HintService {
    /**
     * @param {AiEngine} aiEngine
     */
    constructor(aiEngine) {
        /** @private */
        this._aiEngine = aiEngine;
        /** @private */
        this._token = 0;
        /** @private */
        this._scheduled = false;
        /** @private */
        this._lastBoardKey = null;
    }

    /**
     * ヒント計算をキャンセル
     */
    cancel() {
        this._token += 1;
        this._scheduled = false;
        this._lastBoardKey = null;
    }

    /**
     * ヒント計算をスケジュール
     * @param {Board} board 対象の盤面
     * @param {number} level 計算レベル
     * @param {boolean} isAiTurn AIの手番かどうか
     * @param {(scores: number[] | null) => void} onUpdate スコア更新時のコールバック
     */
    scheduleRefresh(board, level, isAiTurn, onUpdate) {
        if (!this._aiEngine.isReady) return;
        if (isAiTurn) return; // AIの手番中はヒント不要
        if (this._scheduled) return;

        const key = board.toKey();
        if (this._lastBoardKey === key) return;

        this._scheduled = true;
        queueMicrotask(() => {
            this._scheduled = false;
            this._startJob(board, level, onUpdate);
        });
    }

    /**
     * ヒント計算を実行
     * @private
     * @param {Board} board
     * @param {number} level
     * @param {(scores: number[] | null) => void} onUpdate
     */
    async _startJob(board, level, onUpdate) {
        if (!this._aiEngine.isReady) return;

        this.cancel();
        const token = this._token;
        const targetKey = board.toKey();
        this._lastBoardKey = targetKey;

        const legalBits = board.legalMovesBits;
        if (legalBits === 0n) {
            onUpdate(null);
            return;
        }

        const moves = board.legalMovePositions;
        /** @type {(number | null)[]} */
        const scores = Array(64).fill(null);
        onUpdate([...scores]);

        const shouldCancel = () => {
            return token !== this._token;
        };

        const sortByScoreDesc = () => {
            moves.sort((a, b) => {
                const sa = scores[a];
                const sb = scores[b];
                if (sa === null) return 1;
                if (sb === null) return -1;
                return sb - sa;
            });
        };

        // 反復深化: レベル1から指定レベルまで
        for (let lv = 1; lv <= level; lv++) {
            sortByScoreDesc();

            for (const pos of moves) {
                if (shouldCancel()) return;

                try {
                    // 着手を適用して相手視点で評価
                    const { nextPlayer, nextOpponent } = Board.applyMoveForTurn(
                        board.playerBits,
                        board.opponentBits,
                        pos
                    );

                    const { eval: evalScore } = await this._aiEngine.solveTurn(
                        nextOpponent,
                        nextPlayer,
                        lv
                    );

                    if (shouldCancel()) return;

                    // 相手視点の評価を反転して自分視点にする
                    scores[pos] = -evalScore;
                    onUpdate([...scores]);

                    // UIの更新を待つ
                    await new Promise(requestAnimationFrame);
                } catch (error) {
                    if (shouldCancel()) return;
                    console.error('Hint eval failed', error);
                }
            }
        }

        if (token === this._token) {
            this._lastBoardKey = targetKey;
        }
    }

}
