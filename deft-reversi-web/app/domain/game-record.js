/**
 * @fileoverview GameRecord - 棋譜（着手履歴）を管理するクラス
 */

import { positionToNotation } from './types.js';

/**
 * 棋譜（着手履歴）を管理するクラス
 * 不変オブジェクトとして実装
 */
export class GameRecord {
    /**
     * @param {string[]} moves 着手の配列（"f5", "d6", "pass" など）
     */
    constructor(moves = []) {
        /** @private @readonly */
        this._moves = Object.freeze([...moves]);
        Object.freeze(this);
    }

    /** @returns {ReadonlyArray<string>} 着手の配列 */
    get moves() {
        return this._moves;
    }

    /** @returns {number} 着手数 */
    get length() {
        return this._moves.length;
    }

    /**
     * 着手を追加した新しい棋譜を返す
     * @param {number} position 着手位置（0-63）
     * @returns {GameRecord}
     */
    addMove(position) {
        return new GameRecord([...this._moves, positionToNotation(position)]);
    }

    /**
     * パスを追加した新しい棋譜を返す
     * @returns {GameRecord}
     */
    addPass() {
        return new GameRecord([...this._moves, 'pass']);
    }

    /**
     * 棋譜文字列（スペース区切り）
     * @returns {string}
     */
    toString() {
        return this._moves.join(' ');
    }

    /**
     * 等価性判定
     * @param {GameRecord} other
     * @returns {boolean}
     */
    equals(other) {
        if (!(other instanceof GameRecord)) return false;
        if (this._moves.length !== other._moves.length) return false;
        return this._moves.every((move, i) => move === other._moves[i]);
    }
}
