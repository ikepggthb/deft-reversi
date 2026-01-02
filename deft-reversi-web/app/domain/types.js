/**
 * @fileoverview ドメイン層の基本型定義
 */

/**
 * 手番を表す列挙型
 * @readonly
 * @enum {string}
 */
export const Turn = Object.freeze({
    BLACK: 'Black',
    WHITE: 'White',
});

/**
 * 手番を反転する
 * @param {Turn} turn
 * @returns {Turn}
 */
export function oppositeTurn(turn) {
    return turn === Turn.BLACK ? Turn.WHITE : Turn.BLACK;
}

/**
 * 手番を小文字に変換する
 * @param {Turn} turn
 * @returns {'black' | 'white'}
 */
export function turnToLower(turn) {
    return turn.toLowerCase();
}

const LETTERS = 'abcdefgh';
const NUMBERS = '12345678';

/**
 * 位置から座標文字列への変換（例: 37 -> "f5"）
 * @param {number} pos
 * @returns {string}
 */
export function positionToNotation(pos) {
    const col = pos % 8;
    const row = Math.floor(pos / 8);
    return `${LETTERS[col]}${NUMBERS[row]}`;
}

/**
 * 座標文字列から位置への変換（例: "f5" -> 37）
 * @param {string} notation
 * @returns {number | null}
 */
export function notationToPosition(notation) {
    if (notation.length !== 2) return null;
    const col = LETTERS.indexOf(notation[0].toLowerCase());
    const row = NUMBERS.indexOf(notation[1]);
    if (col === -1 || row === -1) return null;
    return row * 8 + col;
}
