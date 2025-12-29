/**
 * @fileoverview AiEngine - Web Worker + WASM のAIエンジン呼び出しを抽象化する
 *
 * - Worker RPC（requestId / timeout）
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
 * Web Workerとの通信を抽象化し、リクエスト/レスポンスパターンを実装するラッパークラス。
 * 各リクエストに一意のIDを付与し、Promiseベースの非同期処理を可能にします。
 */
class WorkerRpc {
    /**
     * @param {URL} workerUrl
     */
    constructor(workerUrl) {
        this.worker = new Worker(workerUrl, { type: 'module' });
        this.pendingRequests = new Map();
        this.generateRequestId = () => `${Date.now()}-${Math.random()}`;
        this.attachMessageHandler();
    }

    /** @private */
    attachMessageHandler() {
        this.worker.addEventListener('message', (event) => {
            if (!event?.data || typeof event.data !== 'object') return;

            const { payload, requestId, ok, error } = event.data;
            if (!requestId) return;

            const pending = this.pendingRequests.get(requestId);
            if (!pending) return;

            if (ok === false) {
                pending.reject(new Error(error ?? 'Unknown worker error'));
            } else {
                pending.resolve(payload);
            }
            this.pendingRequests.delete(requestId);
        });
    }

    /**
     * Workerにリクエストを送信し、レスポンスを待つPromiseを返す
     * @param {string} type
     * @param {any} payload
     * @returns {Promise<any>}
     */
    sendRequest(type, payload) {
        return new Promise((resolve, reject) => {
            const requestId = this.generateRequestId();
            const timeoutId = setTimeout(() => {
                this.pendingRequests.delete(requestId);
                reject(new Error(`Worker request timeout: ${type}`));
            }, DEFAULT_TIMEOUT_MS);

            this.pendingRequests.set(requestId, {
                resolve: (v) => {
                    clearTimeout(timeoutId);
                    resolve(v);
                },
                reject: (e) => {
                    clearTimeout(timeoutId);
                    reject(e);
                },
            });

            // engine/engine.js は payload を配列として受け取り、先頭要素に実データを期待する
            this.worker.postMessage({ type, payload: [payload], requestId });
        });
    }

    terminate() {
        this.worker.terminate();
        this.pendingRequests.clear();
    }
}

/**
 * AI計算結果
 * @typedef {Object} SolverResult
 * @property {number | null} bestMove 最善手の位置（0-63）、パスの場合はnull
 * @property {number} eval 評価値
 */

/**
 * Web Worker + WASM のAIエンジン呼び出しを抽象化するクラス
 */
export class AiEngine {
    constructor() {
        /** @private */
        this._rpc = null;
        /** @private */
        this._ready = false;
        /** @private */
        this._onReadyCallbacks = [];
        /** @private */
        this._readyHandler = null;
    }

    /**
     * エンジンを初期化（Workerを起動）
     * @param {Function} [onReady] 準備完了時のコールバック
     * @param {Function} [onError] エラー時のコールバック
     */
    initialize(onReady, onError) {
        this.terminate();

        this._rpc = new WorkerRpc(new URL('../engine/engine.js', import.meta.url));
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
        this._rpc.worker.addEventListener('message', this._readyHandler);
    }

    /** @returns {boolean} エンジンが準備完了しているか */
    get isReady() {
        return this._ready;
    }

    /**
     * 準備完了を待つ
     * @returns {Promise<void>}
     */
    waitForReady() {
        if (this._ready) return Promise.resolve();
        return new Promise((resolve) => {
            this._onReadyCallbacks.push(resolve);
        });
    }

    /**
     * 最善手を計算する
     * @param {bigint} playerBits プレイヤーのビットボード
     * @param {bigint} opponentBits 相手のビットボード
     * @param {number} level AIレベル（1-24）
     * @returns {Promise<SolverResult>}
     */
    async solveTurn(playerBits, opponentBits, level) {
        if (!this._ready || !this._rpc) {
            throw new Error('Engine not ready');
        }

        const { low: player_bits_low, high: player_bits_high } = bitsToParts(playerBits);
        const { low: opponent_bits_low, high: opponent_bits_high } = bitsToParts(opponentBits);

        const result = await this._rpc.sendRequest('solveTurn', {
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
        if (this._rpc) {
            if (this._readyHandler) {
                this._rpc.worker.removeEventListener('message', this._readyHandler);
                this._readyHandler = null;
            }
            this._rpc.terminate();
            this._rpc = null;
        }
        this._ready = false;
        this._onReadyCallbacks = [];
    }
}

