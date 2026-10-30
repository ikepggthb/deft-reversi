import init, { WasmEngine, WasmPosition } from '../pkg/deft_reversi_wasm_engine.js';

let engine;
self.onmessage = async ({ data }) => {
    try {
        if (data.type === 'init') {
            await init();
            const response = await fetch(new URL('../assets/eval.dpr2.gz', import.meta.url));
            if (!response.ok) throw new Error(`評価モデルの取得に失敗しました (${response.status})`);
            if (!globalThis.DecompressionStream) throw new Error('このブラウザはgzipの展開に対応していません');
            const bytes = new Uint8Array(await new Response(response.body.pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
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
