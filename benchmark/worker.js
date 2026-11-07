import init, { WasmEngine, WasmPosition } from '../pkg/deft_reversi_wasm_engine.js';
import { loadModel } from '../engine/model-loader.js';

let engine;
let resolveConsent;
function requestConsent(info) {
    return new Promise(resolve => { resolveConsent = resolve; self.postMessage({ type: 'consent', info }); });
}
self.onmessage = async ({ data }) => {
    try {
        if (data.type === 'consent-response') { resolveConsent?.(Boolean(data.allowed)); resolveConsent = null; return; }
        if (data.type === 'init') {
            await init();
            const bytes = await loadModel(data.model, progress => self.postMessage({ type: 'progress', progress }), requestConsent);
            engine = new WasmEngine(bytes, data.ttMiB);
            if (data.model === 'classic') engine.useLegacyCalibration();
            self.postMessage({ type: 'ready' });
        } else if (data.type === 'solve') {
            if (!engine) throw new Error('エンジンが初期化されていません');
            engine.clearTt();
            const position = WasmPosition.fromString(data.position);
            try {
                const result = engine.search(position, data.request);
                self.postMessage({ type: 'result', number: data.number, result });
            } finally {
                position.free();
            }
        } else {
            throw new Error('不明なリクエストです');
        }
    } catch (error) {
        self.postMessage({ type: 'error', cancelled: error?.name === 'ModelDownloadCancelled', message: error?.message ?? String(error) });
    }
};
