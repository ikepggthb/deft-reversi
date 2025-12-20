const DEFAULT_TIMEOUT_MS = 120000;

/**
 * Web Workerとの通信を抽象化し、リクエスト/レスポンスパターンを実装するラッパークラス。
 * 各リクエストに一意のIDを付与し、Promiseベースの非同期処理を可能にします。
 */
class WorkerWrapper {
    /**
     * @param {string} workerFile Web Workerとして実行するJavaScriptファイルのパス。
     */
    constructor(workerFile) {
        this.worker = new Worker(workerFile, { type: "module" });
        this.pendingRequests = new Map();
        this.generateRequestId = () => `${Date.now()}-${Math.random()}`;
        this.attachMessageHandler();
    }

    /**
     * Workerからのメッセージを処理するイベントリスナーをセットアップします。
     * レスポンスを対応する保留中のリクエストPromiseに解決または拒否します。
     * @private
     */
    attachMessageHandler() {
        this.worker.addEventListener("message", (event) => {
            if (!event?.data || typeof event.data !== "object") return;
            const { payload, requestId, ok, error } = event.data;
            if (!requestId) return;

            const pending = this.pendingRequests.get(requestId);
            if (!pending) return;

            if (ok === false) {
                pending.reject(new Error(error ?? "Unknown worker error"));
            } else {
                pending.resolve(payload);
            }
            this.pendingRequests.delete(requestId);
        });
    }

    /**
     * Workerにリクエストを送信し、レスポンスを待つPromiseを返します。
     * @param {string} type リクエストの種類を示す文字列。
     * @param {...any} payload Workerに送信するデータ。
     * @returns {Promise<any>} Workerからのレスポンスペイロードで解決されるPromise。
     */
    sendRequest(type, ...payload) {
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
            this.worker.postMessage({ type, payload, requestId });
        });
    }
}

/**
 * リバーシの思考エンジン(Web Worker)と通信するためのクライアントクラス。
 * WorkerWrapperを継承し、エンジン固有のメソッド（例: solveTurn）を提供します。
 */
export class Engine extends WorkerWrapper {
    constructor() {
        super("engine.js");
        this.initializeMethods(["solveTurn"]);
    }

    /**
     * 指定されたメソッド名の配列に基づいて、インスタンスにメソッドを動的に生成します。
     * 各メソッドはsendRequestを呼び出すラッパーとなります。
     * @param {string[]} methods 生成するメソッド名の配列。
     * @private
     */
    initializeMethods(methods) {
        methods.forEach((method) => {
            /**
             * Workerにリクエストを送信します。
             * @param {...any} payload Workerに送信するデータ。
             * @returns {Promise<any>} Workerからのレスポンス。
             */
            this[method] = (...payload) => this.sendRequest(method, ...payload);
        });
    }
}