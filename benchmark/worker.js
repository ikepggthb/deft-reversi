import init, { WasmEngine, WasmPosition } from '../pkg/deft_reversi_wasm_engine.js';
import { loadModel } from '../engine/model-loader.js';

let engine;
self.onmessage = async ({ data }) => {
    try {
        if (data.type === 'init') {
            await init();
            const bytes = await loadModel(data.model, progress => self.postMessage({ type: 'progress', progress }));
            engine = new WasmEngine(bytes, data.ttMiB);
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
        self.postMessage({ type: 'error', message: error?.message ?? String(error) });
    }
};
