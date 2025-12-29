/**
 * @fileoverview Game - リバーシゲームの集約ルート
 */

import { Board } from './board.js';
import { GameRecord } from './game-record.js';
import { Turn } from './types.js';

/**
 * ゲームのスナップショット（値オブジェクト）
 * @typedef {Object} GameSnapshot
 * @property {bigint} blackBits
 * @property {bigint} whiteBits
 * @property {Turn} nextTurn
 * @property {string[]} recordMoves
 * @property {number | null} lastMove
 */

/**
 * リバーシゲームの集約ルート
 * 盤面状態、棋譜、最後の着手を管理
 */
export class Game {
    /**
     * @param {Board} board
     * @param {GameRecord} record
     * @param {number | null} lastMove
     */
    constructor(board, record = new GameRecord(), lastMove = null) {
        /** @private @readonly */
        this._board = board;
        /** @private @readonly */
        this._record = record;
        /** @private @readonly */
        this._lastMove = lastMove;
        Object.freeze(this);
    }

    /**
     * 新規ゲームを開始
     * @returns {Game}
     */
    static newGame() {
        return new Game(Board.initial(), new GameRecord(), null);
    }

    /** @returns {Board} 現在の盤面 */
    get board() {
        return this._board;
    }

    /** @returns {GameRecord} 棋譜 */
    get record() {
        return this._record;
    }

    /** @returns {number | null} 最後の着手位置 */
    get lastMove() {
        return this._lastMove;
    }

    /** @returns {Turn} 次の手番 */
    get nextTurn() {
        return this._board.nextTurn;
    }

    /** @returns {boolean} ゲーム終了か */
    get isGameOver() {
        return this._board.isGameOver();
    }

    /** @returns {boolean} パスすべきか */
    get mustPass() {
        return this._board.mustPass();
    }

    /** @returns {bigint} 合法手のビットボード */
    get legalMovesBits() {
        return this._board.legalMovesBits;
    }

    /** @returns {number[]} 合法手の位置配列 */
    get legalMovePositions() {
        return this._board.legalMovePositions;
    }

    /**
     * 指定位置に着手できるか
     * @param {number} position
     * @returns {boolean}
     */
    canPlace(position) {
        return this._board.canPlace(position);
    }

    /**
     * 着手を適用して新しいGameを返す
     * @param {number} position 着手位置
     * @returns {Game}
     * @throws {Error} 非合法手の場合
     */
    applyMove(position) {
        const newBoard = this._board.applyMove(position);
        const newRecord = this._record.addMove(position);
        return new Game(newBoard, newRecord, position);
    }

    /**
     * パスを適用して新しいGameを返す
     * @returns {Game}
     */
    applyPass() {
        const newBoard = this._board.applyPass();
        const newRecord = this._record.addPass();
        return new Game(newBoard, newRecord, this._lastMove);
    }

    /**
     * スナップショットを取得（シリアライズ可能な形式）
     * @returns {GameSnapshot}
     */
    snapshot() {
        return {
            blackBits: this._board.blackBits,
            whiteBits: this._board.whiteBits,
            nextTurn: this._board.nextTurn,
            recordMoves: [...this._record.moves],
            lastMove: this._lastMove,
        };
    }

    /**
     * スナップショットから復元
     * @param {GameSnapshot} snapshot
     * @returns {Game}
     */
    static fromSnapshot(snapshot) {
        const board = Board.fromBits(snapshot.blackBits, snapshot.whiteBits, snapshot.nextTurn);
        const record = new GameRecord(snapshot.recordMoves);
        return new Game(board, record, snapshot.lastMove);
    }

    /**
     * 結果を取得（ゲーム終了時のみ有効）
     * @returns {{ blackCount: number, whiteCount: number, winner: Turn | null }}
     */
    getResult() {
        const blackCount = this._board.blackCount;
        const whiteCount = this._board.whiteCount;
        let winner = null;
        if (blackCount > whiteCount) {
            winner = Turn.BLACK;
        } else if (whiteCount > blackCount) {
            winner = Turn.WHITE;
        }
        return { blackCount, whiteCount, winner };
    }

    /**
     * 状態の一意キー
     * @returns {string}
     */
    toKey() {
        return this._board.toKey();
    }
}
