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
