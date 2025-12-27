/**
 * ビットが立っている数を数えます。BigIntのみを受け付けます。
 * @param {bigint | null | undefined} bits ビット数を数える対象のBigInt。
 * @returns {number} 立っているビットの数。
 */
export function countBits(bits) {
    if (bits === null || bits === undefined) return 0;
    if (typeof bits !== "bigint") {
        console.error("countBits expects BigInt but got", typeof bits);
        return 0;
    }
    let x = bits < 0n ? -bits : bits;
    let c = 0;
    while (x !== 0n) {
        x &= x - 1n;
        c++;
    }
    return c;
}

/**
 * 2つの32ビット数値（下位および上位）から64ビットのBigIntを構築します。
 * @param {number} low BigIntの下位32ビットを表す数値。
 * @param {number} high BigIntの上位32ビットを表す数値。
 * @returns {bigint | null} 構築されたBigInt、または入力が無効な場合はnull。
 */
export function bitsFromParts(low, high) {
    if (typeof low !== "number" || typeof high !== "number") return null;
    return (BigInt(high >>> 0) << 32n) | BigInt(low >>> 0);
}

/**
 * ゲーム状態オブジェクトから指定されたキーのビットボード（BigInt）を取得します。
 * @param {object} status ゲーム状態オブジェクト。`${key}_bits_low`と`${key}_bits_high`プロパティを持つことを期待します。
 * @param {string} key 取得するビットボードの種類を示すキー（例: "black", "white", "legal_moves"）。
 * @returns {bigint | null} 取得されたビットボード。
 */
export function getBits(status, key) {
    const low = status?.[`${key}_bits_low`];
    const high = status?.[`${key}_bits_high`];
    return bitsFromParts(low, high);
}

/**
 * 64ビットのBigIntを2つの32ビット数値（下位および上位）に分割します。
 * @param {bigint} bits 分割するBigInt。
 * @returns {{low: number, high: number}} 下位および上位の32ビット数値を含むオブジェクト。
 */
export function bitsToParts(bits) {
    if (typeof bits !== "bigint") return { low: 0, high: 0 };
    return {
        low: Number(bits & 0xffff_ffffn),
        high: Number((bits >> 32n) & 0xffff_ffffn),
    };
}

const FULL_MASK = 0xffff_ffff_ffff_ffffn;
const NOT_A_FILE = 0xfefefefefefefefen;
const NOT_H_FILE = 0x7f7f7f7f7f7f7f7fn;

function shiftE(x) {
    return ((x & NOT_H_FILE) << 1n) & FULL_MASK;
}
function shiftW(x) {
    return ((x & NOT_A_FILE) >> 1n) & FULL_MASK;
}
function shiftN(x) {
    return (x << 8n) & FULL_MASK;
}
function shiftS(x) {
    return (x >> 8n) & FULL_MASK;
}
function shiftNE(x) {
    return ((x & NOT_H_FILE) << 9n) & FULL_MASK;
}
function shiftNW(x) {
    return ((x & NOT_A_FILE) << 7n) & FULL_MASK;
}
function shiftSE(x) {
    return ((x & NOT_H_FILE) >> 7n) & FULL_MASK;
}
function shiftSW(x) {
    return ((x & NOT_A_FILE) >> 9n) & FULL_MASK;
}

/**
 * 指定されたプレイヤーと相手の盤面から、現在のプレイヤーの合法手を計算します。
 * @param {bigint} playerBits 現在のプレイヤーの石の位置を示すビットボード。
 * @param {bigint} opponentBits 相手プレイヤーの石の位置を示すビットボード。
 * @returns {bigint} 合法手が存在するすべての位置を示すビットボード。
 */
export function legalMoves(playerBits, opponentBits) {
    let moves = 0n;
    const p = playerBits & FULL_MASK;
    const o = opponentBits & FULL_MASK;
    const empty = ~(p | o) & FULL_MASK;
    const shifts = [shiftN, shiftS, shiftE, shiftW, shiftNE, shiftNW, shiftSE, shiftSW];

    for (const sh of shifts) {
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
 * 指定された位置に石を置いたときに裏返る相手の石を計算します。
 * @param {bigint} posMask 石を置く位置を示すビットマスク (1n << position)。
 * @param {bigint} playerBits 現在のプレイヤーの石の位置を示すビットボード。
 * @param {bigint} opponentBits 相手プレイヤーの石の位置を示すビットボード。
 * @returns {bigint} 裏返るすべての石の位置を示すビットボード。
 */
export function flipBits(posMask, playerBits, opponentBits) {
    let flipped = 0n;
    const p = playerBits & FULL_MASK;
    const o = opponentBits & FULL_MASK;
    const shifts = [shiftN, shiftS, shiftE, shiftW, shiftNE, shiftNW, shiftSE, shiftSW];

    for (const sh of shifts) {
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
 * プレイヤーの着手を適用し、着手後の盤面状態を計算します。
 * @param {bigint} playerBits 現在のプレイヤーの石の位置を示すビットボード。
 * @param {bigint} opponentBits 相手プレイヤーの石の位置を示すビットボード。
 * @param {number} pos 石を置く位置 (0-63)。
 * @throws {Error} 指定された手が非合法手の場合にエラーをスローします。
 * @returns {{nextPlayer: bigint, nextOpponent: bigint, flipped: bigint}} 更新されたプレイヤーと相手の盤面、および裏返された石の位置を含むオブジェクト。
 */
export function applyMoveForTurn(playerBits, opponentBits, pos) {
    const posMask = 1n << BigInt(pos);
    if ((posMask & legalMoves(playerBits, opponentBits)) === 0n) {
        throw new Error("illegal move");
    }
    const flipped = flipBits(posMask, playerBits, opponentBits);
    const nextPlayer = playerBits ^ flipped ^ posMask;
    const nextOpponent = opponentBits ^ flipped;
    return { nextPlayer, nextOpponent, flipped };
}

/**
 * ゲームが終了したかどうかを判定します。
 * 両プレイヤーがともに行える手がない場合に終了とみなします。
 * @param {bigint} playerBits プレイヤー1の石の位置を示すビットボード。
 * @param {bigint} opponentBits プレイヤー2の石の位置を示すビットボード。
 * @returns {boolean} ゲームが終了している場合はtrue、そうでない場合はfalse。
 */
export function isEnd(playerBits, opponentBits) {
    const moves = legalMoves(playerBits, opponentBits);
    const oppMoves = legalMoves(opponentBits, playerBits);
    return moves === 0n && oppMoves === 0n;
}

/**
 * 現在のプレイヤーがパスしなければならない状況かどうかを判定します。
 * @param {bigint} playerBits 現在のプレイヤーの石の位置を示すビットボード。
 * @param {bigint} opponentBits 相手プレイヤーの石の位置を示すビットボード。
 * @returns {boolean} 現在のプレイヤーがパスすべき場合はtrue、そうでない場合はfalse。
 */
export function isPass(playerBits, opponentBits) {
    const moves = legalMoves(playerBits, opponentBits);
    const oppMoves = legalMoves(opponentBits, playerBits);
    return moves === 0n && oppMoves !== 0n;
}
