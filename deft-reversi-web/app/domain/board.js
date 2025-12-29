/**
 * @fileoverview Board値オブジェクト - リバーシの盤面を表す不変オブジェクト
 * ビットボード演算ロジックを含む
 */

import { Turn, oppositeTurn } from './types.js';

/**
 * 初期配置
 */
const INITIAL_BLACK = 0x0000000810000000n;
const INITIAL_WHITE = 0x0000001008000000n;

// ビットボード演算用定数
const FULL_MASK = 0xffff_ffff_ffff_ffffn;
const NOT_A_FILE = 0xfefefefefefefefen;
const NOT_H_FILE = 0x7f7f7f7f7f7f7f7fn;

// シフト関数（8方向）
const shiftE = (x) => ((x & NOT_H_FILE) << 1n) & FULL_MASK;
const shiftW = (x) => ((x & NOT_A_FILE) >> 1n) & FULL_MASK;
const shiftN = (x) => (x << 8n) & FULL_MASK;
const shiftS = (x) => (x >> 8n) & FULL_MASK;
const shiftNE = (x) => ((x & NOT_H_FILE) << 9n) & FULL_MASK;
const shiftNW = (x) => ((x & NOT_A_FILE) << 7n) & FULL_MASK;
const shiftSE = (x) => ((x & NOT_H_FILE) >> 7n) & FULL_MASK;
const shiftSW = (x) => ((x & NOT_A_FILE) >> 9n) & FULL_MASK;
const SHIFTS = [shiftN, shiftS, shiftE, shiftW, shiftNE, shiftNW, shiftSE, shiftSW];

/**
 * リバーシの盤面を表す不変（Immutable）な値オブジェクト
 * ビットボードを内部表現として持つ
 */
export class Board {
    /**
     * @param {bigint} blackBits 黒石のビットボード
     * @param {bigint} whiteBits 白石のビットボード
     * @param {Turn} nextTurn 次の手番
     */
    constructor(blackBits, whiteBits, nextTurn) {
        /** @private @readonly */
        this._blackBits = blackBits;
        /** @private @readonly */
        this._whiteBits = whiteBits;
        /** @private @readonly */
        this._nextTurn = nextTurn;
        // 合法手はコンストラクタで計算してキャッシュ（freezeする前に）
        const player = nextTurn === Turn.BLACK ? blackBits : whiteBits;
        const opponent = nextTurn === Turn.BLACK ? whiteBits : blackBits;
        /** @private @readonly */
        this._legalMovesBits = Board.legalMoves(player, opponent);
        Object.freeze(this);
    }

    /**
     * 初期盤面を生成する
     * @returns {Board}
     */
    static initial() {
        return new Board(INITIAL_BLACK, INITIAL_WHITE, Turn.BLACK);
    }

    /**
     * ビットボードから盤面を生成する
     * @param {bigint} blackBits
     * @param {bigint} whiteBits
     * @param {Turn} nextTurn
     * @returns {Board}
     */
    static fromBits(blackBits, whiteBits, nextTurn) {
        return new Board(blackBits, whiteBits, nextTurn);
    }

    // === 静的メソッド: ビットボード演算 ===

    /**
     * ビットが立っている数を数える
     * @param {bigint | null | undefined} bits
     * @returns {number}
     */
    static countBits(bits) {
        if (bits === null || bits === undefined) return 0;
        if (typeof bits !== 'bigint') return 0;
        let x = bits < 0n ? -bits : bits;
        let c = 0;
        while (x !== 0n) {
            x &= x - 1n;
            c++;
        }
        return c;
    }

    /**
     * 合法手を計算する
     * @param {bigint} playerBits 現在のプレイヤーの石
     * @param {bigint} opponentBits 相手プレイヤーの石
     * @returns {bigint} 合法手のビットボード
     */
    static legalMoves(playerBits, opponentBits) {
        let moves = 0n;
        const p = playerBits & FULL_MASK;
        const o = opponentBits & FULL_MASK;
        const empty = ~(p | o) & FULL_MASK;

        for (const sh of SHIFTS) {
            let t = sh(p) & o;
            t |= sh(t) & o;
            t |= sh(t) & o;
            t |= sh(t) & o;
            t |= sh(t) & o;
            t |= sh(t) & o;
            moves |= sh(t) & empty;
        }
        return moves;
    }

    /**
     * 裏返る石を計算する
     * @param {bigint} posMask 着手位置のビットマスク
     * @param {bigint} playerBits 現在のプレイヤーの石
     * @param {bigint} opponentBits 相手プレイヤーの石
     * @returns {bigint} 裏返る石のビットボード
     */
    static flipBits(posMask, playerBits, opponentBits) {
        let flipped = 0n;
        const p = playerBits & FULL_MASK;
        const o = opponentBits & FULL_MASK;

        for (const sh of SHIFTS) {
            let acc = 0n;
            let t = sh(posMask & FULL_MASK);
            while (t !== 0n && (t & o) !== 0n) {
                acc |= t;
                t = sh(t);
            }
            if ((t & p) !== 0n) flipped |= acc;
        }
        return flipped;
    }

    /**
     * 着手を適用して次の盤面状態を計算する
     * @param {bigint} playerBits 現在のプレイヤーの石
     * @param {bigint} opponentBits 相手プレイヤーの石
     * @param {number} pos 着手位置 (0-63)
     * @returns {{nextPlayer: bigint, nextOpponent: bigint, flipped: bigint}}
     * @throws {Error} 非合法手の場合
     */
    static applyMoveForTurn(playerBits, opponentBits, pos) {
        const posMask = 1n << BigInt(pos);
        if ((posMask & Board.legalMoves(playerBits, opponentBits)) === 0n) {
            throw new Error('illegal move');
        }
        const flipped = Board.flipBits(posMask, playerBits, opponentBits);
        const nextPlayer = playerBits ^ flipped ^ posMask;
        const nextOpponent = opponentBits ^ flipped;
        return { nextPlayer, nextOpponent, flipped };
    }

    /**
     * ゲームが終了したかどうかを判定
     * @param {bigint} playerBits
     * @param {bigint} opponentBits
     * @returns {boolean}
     */
    static isEnd(playerBits, opponentBits) {
        const moves = Board.legalMoves(playerBits, opponentBits);
        const oppMoves = Board.legalMoves(opponentBits, playerBits);
        return moves === 0n && oppMoves === 0n;
    }

    /**
     * パスすべきかどうかを判定
     * @param {bigint} playerBits
     * @param {bigint} opponentBits
     * @returns {boolean}
     */
    static isPass(playerBits, opponentBits) {
        const moves = Board.legalMoves(playerBits, opponentBits);
        const oppMoves = Board.legalMoves(opponentBits, playerBits);
        return moves === 0n && oppMoves !== 0n;
    }

    // === インスタンスプロパティ ===

    /** @returns {bigint} 黒石のビットボード */
    get blackBits() {
        return this._blackBits;
    }

    /** @returns {bigint} 白石のビットボード */
    get whiteBits() {
        return this._whiteBits;
    }

    /** @returns {Turn} 次の手番 */
    get nextTurn() {
        return this._nextTurn;
    }

    /** @returns {bigint} 現在手番のプレイヤーのビットボード */
    get playerBits() {
        return this._nextTurn === Turn.BLACK ? this._blackBits : this._whiteBits;
    }

    /** @returns {bigint} 相手プレイヤーのビットボード */
    get opponentBits() {
        return this._nextTurn === Turn.BLACK ? this._whiteBits : this._blackBits;
    }

    /** @returns {bigint} 合法手のビットボード */
    get legalMovesBits() {
        return this._legalMovesBits;
    }

    /** @returns {number[]} 合法手の位置配列 */
    get legalMovePositions() {
        const moves = [];
        const bits = this.legalMovesBits;
        for (let i = 0; i < 64; i++) {
            if ((bits >> BigInt(i)) & 1n) {
                moves.push(i);
            }
        }
        return moves;
    }

    /** @returns {number} 黒石の数 */
    get blackCount() {
        return Board.countBits(this._blackBits);
    }

    /** @returns {number} 白石の数 */
    get whiteCount() {
        return Board.countBits(this._whiteBits);
    }

    /** @returns {number} 空きマスの数 */
    get emptyCount() {
        return 64 - this.blackCount - this.whiteCount;
    }

    /**
     * 指定位置に着手できるか
     * @param {number} position
     * @returns {boolean}
     */
    canPlace(position) {
        const mask = 1n << BigInt(position);
        return (this.legalMovesBits & mask) !== 0n;
    }

    /**
     * 着手を適用して新しい盤面を返す（不変）
     * @param {number} position 着手位置
     * @returns {Board}
     * @throws {Error} 非合法手の場合
     */
    applyMove(position) {
        const { nextPlayer, nextOpponent } = Board.applyMoveForTurn(
            this.playerBits,
            this.opponentBits,
            position
        );
        const newNextTurn = oppositeTurn(this._nextTurn);
        if (this._nextTurn === Turn.BLACK) {
            return new Board(nextPlayer, nextOpponent, newNextTurn);
        } else {
            return new Board(nextOpponent, nextPlayer, newNextTurn);
        }
    }

    /**
     * パスを適用して新しい盤面を返す
     * @returns {Board}
     */
    applyPass() {
        return new Board(this._blackBits, this._whiteBits, oppositeTurn(this._nextTurn));
    }

    /** @returns {boolean} 現在手番がパスすべきか */
    mustPass() {
        return Board.isPass(this.playerBits, this.opponentBits);
    }

    /** @returns {boolean} ゲーム終了か */
    isGameOver() {
        return Board.isEnd(this.playerBits, this.opponentBits);
    }

    /**
     * 状態の一意キー（ヒント計算のキャンセル判定用）
     * @returns {string}
     */
    toKey() {
        return `${this._blackBits}:${this._whiteBits}:${this._nextTurn}`;
    }

    /**
     * 等価性判定
     * @param {Board} other
     * @returns {boolean}
     */
    equals(other) {
        if (!(other instanceof Board)) return false;
        return (
            this._blackBits === other._blackBits &&
            this._whiteBits === other._whiteBits &&
            this._nextTurn === other._nextTurn
        );
    }

    /**
     * デバッグ用の盤面文字列表現
     * @returns {string}
     */
    toString() {
        let result = '  a b c d e f g h\n';
        for (let row = 0; row < 8; row++) {
            result += `${row + 1} `;
            for (let col = 0; col < 8; col++) {
                const pos = row * 8 + col;
                const mask = 1n << BigInt(pos);
                if (this._blackBits & mask) {
                    result += 'X ';
                } else if (this._whiteBits & mask) {
                    result += 'O ';
                } else if (this.legalMovesBits & mask) {
                    result += '* ';
                } else {
                    result += '. ';
                }
            }
            result += '\n';
        }
        result += `Next: ${this._nextTurn}, Black: ${this.blackCount}, White: ${this.whiteCount}`;
        return result;
    }
}
