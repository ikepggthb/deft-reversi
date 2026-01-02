/**
 * @fileoverview AiEngine - Web Worker + WASM のAIエンジン呼び出しを管理する
 *
 * - Worker通信（requestId / timeout）
 * - BigInt bitboard ↔ u32 low/high 変換
 * を1箇所に集約し、アプリケーション層をWorker境界の表現から隔離する。
 */

const DEFAULT_TIMEOUT_MS = 120000;

/**
 * BigIntを32ビットペアに変換（Worker通信用）
 * @param {bigint} bits
 * @returns {{low: number, high: number}}
 */
function bitsToParts(bits) {
    if (typeof bits !== 'bigint') return { low: 0, high: 0 };
    return {
        low: Number(bits & 0xffff_ffffn),
        high: Number((bits >> 32n) & 0xffff_ffffn),
    };
}

/**
 * ビットマスク(low/high)から位置(0-63)を抽出
 * @param {number | undefined} low
 * @param {number | undefined} high
 * @returns {number | null}
 */
function extractPosition(low, high) {
    const l = low ?? 0;
    const h = high ?? 0;
    const mask = (BigInt(h >>> 0) << 32n) | BigInt(l >>> 0);
    if (mask === 0n) return null;

    let pos = 0;
    let m = mask;
    while ((m & 1n) === 0n) {
        m >>= 1n;
        pos++;
    }
    return pos;
}

/**
 * AI計算結果
 * @typedef {Object} SolverResult
 * @property {number | null} bestMove 最善手の位置（0-63）、パスの場合はnull
 * @property {number} eval 評価値
 */

/**
 * Web Worker + WASM のAIエンジン呼び出しを管理するクラス
 */
export class AiEngine {
    constructor() {
        /** @private */
        this._worker = null;
        /** @private */
        this._ready = false;
        /** @private */
        this._onReadyCallbacks = [];
        /** @private */
        this._readyHandler = null;
        /** @private */
        this._pendingRequests = new Map();
    }

    /**
     * エンジンを初期化（Workerを起動）
     * @param {Function} [onReady] 準備完了時のコールバック
     * @param {Function} [onError] エラー時のコールバック
     */
    initialize(onReady, onError) {
        this.terminate();

        this._worker = new Worker(new URL('../engine/engine.js', import.meta.url), {
            type: 'module',
        });
        this._ready = false;

        this._readyHandler = (event) => {
            const data = event.data;
            if (data?.type !== 'ready') return;

            if (data.ok) {
                this._ready = true;
                onReady?.();
                this._onReadyCallbacks.forEach((cb) => cb());
                this._onReadyCallbacks = [];
            } else {
                onError?.(new Error(data.error ?? 'Engine initialization failed'));
            }
        };
        this._worker.addEventListener('message', this._readyHandler);
        this._attachMessageHandler();
    }

    /**
     * メッセージハンドラーを設定
     * @private
     */
    _attachMessageHandler() {
        this._worker.addEventListener('message', (event) => {
            const data = event?.data;
            if (!data || typeof data !== 'object') return;
            if (!data.requestId) return;

            const pending = this._pendingRequests.get(data.requestId);
            if (!pending) return;

            if (data.ok === false) {
                pending.reject(new Error(data.error ?? 'Unknown worker error'));
            } else {
                pending.resolve(data.payload);
            }
            this._pendingRequests.delete(data.requestId);
        });
    }

    /**
     * Workerにリクエストを送信
     * @private
     * @param {string} type
     * @param {any} payload
     * @returns {Promise<any>}
     */
    _sendRequest(type, payload) {
        return new Promise((resolve, reject) => {
            const requestId = `${Date.now()}-${Math.random()}`;
            const timeoutId = setTimeout(() => {
                this._pendingRequests.delete(requestId);
                reject(new Error(`Worker request timeout: ${type}`));
            }, DEFAULT_TIMEOUT_MS);

            this._pendingRequests.set(requestId, {
                resolve: (v) => {
                    clearTimeout(timeoutId);
                    resolve(v);
                },
                reject: (e) => {
                    clearTimeout(timeoutId);
                    reject(e);
                },
            });

            const message = { type, payload: [payload], requestId };
            this._worker.postMessage(message);
        });
    }

    /** @returns {boolean} エンジンが準備完了しているか */
    get isReady() {
        return this._ready;
    }

    /**
     * 最善手を計算する
     * @param {bigint} playerBits プレイヤーのビットボード
     * @param {bigint} opponentBits 相手のビットボード
     * @param {number} level AIレベル（1-24）
     * @returns {Promise<SolverResult>}
     */
    async solveTurn(playerBits, opponentBits, level) {
        if (!this._ready || !this._worker) {
            throw new Error('Engine not ready');
        }

        const { low: player_bits_low, high: player_bits_high } = bitsToParts(playerBits);
        const { low: opponent_bits_low, high: opponent_bits_high } = bitsToParts(opponentBits);

        const result = await this._sendRequest('solveTurn', {
            player_bits_low,
            player_bits_high,
            opponent_bits_low,
            opponent_bits_high,
            aiLevel: level,
        });

        const bestMove = extractPosition(result?.best_move_low, result?.best_move_high);
        const evalScore = typeof result?.eval === 'number' ? result.eval : 0;

        return { bestMove, eval: evalScore };
    }

    /**
     * エンジンを終了
     */
    terminate() {
        if (this._worker) {
            if (this._readyHandler) {
                this._worker.removeEventListener('message', this._readyHandler);
                this._readyHandler = null;
            }
            this._worker.terminate();
            this._worker = null;
        }
        this._ready = false;
        this._onReadyCallbacks = [];
        this._pendingRequests.clear();
    }
}

