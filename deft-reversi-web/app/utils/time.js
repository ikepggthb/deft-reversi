/**
 * 指定されたミリ秒数だけ非同期に待機します。
 * @param {number} timeMs 待機するミリ秒数。
 * @returns {Promise<void>} 指定時間が経過した後に解決されるPromise。
 */
export const sleep = (timeMs) => new Promise((resolve) => setTimeout(resolve, timeMs));

