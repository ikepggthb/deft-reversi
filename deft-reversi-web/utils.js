export const sleep = (timeMs) => new Promise((resolve) => setTimeout(resolve, timeMs));

/**
 * Count number of set bits. BigIntのみを受け付ける（null/undefinedは0）。
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

export function bitsFromParts(low, high) {
    if (typeof low !== "number" || typeof high !== "number") return null;
    return (BigInt(high >>> 0) << 32n) | BigInt(low >>> 0);
}

export function getBits(status, key) {
    const low = status?.[`${key}_bits_low`];
    const high = status?.[`${key}_bits_high`];
    return bitsFromParts(low, high);
}

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

export function isEnd(playerBits, opponentBits) {
    const moves = legalMoves(playerBits, opponentBits);
    const oppMoves = legalMoves(opponentBits, playerBits);
    return moves === 0n && oppMoves === 0n;
}

export function isPass(playerBits, opponentBits) {
    const moves = legalMoves(playerBits, opponentBits);
    const oppMoves = legalMoves(opponentBits, playerBits);
    return moves === 0n && oppMoves !== 0n;
}
