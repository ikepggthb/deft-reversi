import pako from 'https://cdnjs.cloudflare.com/ajax/libs/pako/2.1.0/pako.esm.mjs';
import __wbg_init, { AiSolver } from "./pkg/deft_reversi_web.js";

async function fetch_eval_data() {
    try {
        const response = await fetch('./deft_eval_2024-01-27.json.gz');
        if (!response.ok) {
            throw new Error(response.statusText);
        }

        const data = await response.arrayBuffer();
        const decompressedData = await pako.ungzip(data, { to: 'string' });
        return decompressedData;
    } catch (error) {
        console.log("評価データの読み込みに失敗しました\n" + error);
    }
}

async function init() {
    try {
        await __wbg_init();
        console.log("fetch eval data");
        const eval_data = await fetch_eval_data();
        console.log("init ai solver");
        const ai = new AiSolver(eval_data);
        self.postMessage({ type: "ready", ok: true, payload: null, requestId: null, protocolVersion: 1 });
        return ai;
    } catch (error) {
        console.error("init failed", error);
        self.postMessage({ type: "ready", ok: false, error: error?.message ?? String(error), payload: null, requestId: null, protocolVersion: 1 });
        return undefined;
    }
}

const ai = await init();

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
