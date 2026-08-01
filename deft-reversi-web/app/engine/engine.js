/**
 * @file Reversi AIエンジンのWeb Workerスクリプト。
 * メインスレッドから独立して動作し、WebAssemblyでコンパイルされたAIの思考ルーチンを実行します。
 * Wasmモジュールの初期化、評価データのロード、およびメインスレッドとの通信を処理します。
 */

import __wbg_init, { AiSolver } from "../pkg/deft_reversi_web.js";

/**
 * AIの評価関数で使用されるデータをフェッチし、解凍します。
 * @returns {Promise<string|undefined>} 成功した場合は解凍された評価データ（文字列）、失敗した場合はundefined。
 * @private
 */
async function fetch_eval_data() {
    try {
        const response = await fetch(new URL("../assets/deft_eval_2024-01-27.json.gz", import.meta.url));
        if (!response.ok) {
            throw new Error(response.statusText);
        }

        const data = await response.arrayBuffer();
        const decompressedData = await ungzipToString(data);
        return decompressedData;
    } catch (error) {
        throw new Error(`評価データの読み込みに失敗しました: ${error?.message ?? String(error)}`);
    }
}

/**
 * gzip(ArrayBuffer)を文字列へ解凍する（CDN依存を避けるため DecompressionStream を使用）
 * @param {ArrayBuffer} buffer
 * @returns {Promise<string>}
 */
async function ungzipToString(buffer) {
    if (typeof DecompressionStream !== 'function') {
        throw new Error('DecompressionStream is not supported in this environment');
    }
    const stream = new Blob([buffer]).stream().pipeThrough(new DecompressionStream('gzip'));
    return await new Response(stream).text();
}

/**
 * WebAssemblyモジュールとAIソルバーを初期化します。
 * 成功または失敗のステータスをメインスレッドに通知します。
 * @returns {Promise<AiSolver|undefined>} 初期化に成功した場合はAiSolverインスタンス、失敗した場合はundefined。
 * @private
 */
async function init() {
    try {
        await __wbg_init();
        const eval_data = await fetch_eval_data();
        const ai = new AiSolver(eval_data);
        self.postMessage({ type: "ready", ok: true, payload: null, requestId: null, protocolVersion: 1 });
        return ai;
    } catch (error) {
        console.error("init failed", error);
        self.postMessage({ type: "ready", ok: false, error: error?.message ?? String(error), payload: null, requestId: null, protocolVersion: 1 });
        return undefined;
    }
}

// スクリプトの読み込み時にAIを初期化
const ai = await init();

/**
 * メインスレッドからのメッセージを処理するイベントリスナー。
 * 'solveTurn'のようなリクエストを受け取り、AIソルバーを実行して結果を返します。
 */
self.addEventListener('message', (event) => {
    if (ai === undefined) {
        if (event?.data?.requestId) {
            self.postMessage({ type: event.data.type, ok: false, error: "Engine not initialized", payload: null, requestId: event.data.requestId, protocolVersion: 1 });
        }
        return;
    }
    const { type, payload, requestId } = event.data;

    const responseBase = { requestId, protocolVersion: 1 };
    try {
        const result = (function (){
            switch (type) {
                case 'solveTurn': {
                    const [{ player_bits_low, player_bits_high, opponent_bits_low, opponent_bits_high, aiLevel }] = payload;
                    return ai.solver_result_for_turn_bits(
                        player_bits_low,
                        player_bits_high,
                        opponent_bits_low,
                        opponent_bits_high,
                        aiLevel
                    );
                }
                default:
                    throw new Error(`Unknown message type: ${type}`);
            }
        })();

        self.postMessage({
            ...responseBase,
            type,
            ok: true,
            payload: result
        });
    } catch (error) {
        console.error("engine.js error", error);
        self.postMessage({
            ...responseBase,
            type,
            ok: false,
            error: error?.message ?? String(error),
            payload: null
        });
    }
});
