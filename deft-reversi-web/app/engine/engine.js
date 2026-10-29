import init, { WasmEngine, WasmPosition } from '../pkg/deft_reversi_wasm_engine.js';

async function initialize() {
    await init();
    const response = await fetch(new URL('../assets/eval.dpr2.gz', import.meta.url));
    if (!response.ok) throw new Error(`評価データの読み込みに失敗しました: ${response.status}`);
    const stream = response.body.pipeThrough(new DecompressionStream('gzip'));
    const bytes = new Uint8Array(await new Response(stream).arrayBuffer());
    return new WasmEngine(bytes, 32);
}

let engine;
try {
    engine = await initialize();
    self.postMessage({ type: 'ready', ok: true, protocolVersion: 1 });
} catch (error) {
    self.postMessage({ type: 'ready', ok: false, error: String(error), protocolVersion: 1 });
}

self.addEventListener('message', ({ data }) => {
    const { type, payload, requestId } = data;
    try {
        if (!engine) throw new Error('Engine not initialized');
        if (type !== 'solveTurn') throw new Error(`Unknown message type: ${type}`);
        const p = payload[0];
        const player = BigInt(p.player_bits_low >>> 0) | (BigInt(p.player_bits_high >>> 0) << 32n);
        const opponent = BigInt(p.opponent_bits_low >>> 0) | (BigInt(p.opponent_bits_high >>> 0) << 32n);
        const position = WasmPosition.fromBits(player, opponent);
        let result;
        try { result = engine.search(position, { goal: { kind: 'level', value: p.aiLevel } }); }
        finally { position.free(); }
        let mask = 0n;
        if (result.bestMove && result.bestMove !== 'pass') {
            const move = result.bestMove.toUpperCase();
            mask = 1n << BigInt((move.charCodeAt(1) - 49) * 8 + move.charCodeAt(0) - 65);
        }
        self.postMessage({ type, requestId, protocolVersion: 1, ok: true, payload: {
            best_move_low: Number(mask & 0xffffffffn), best_move_high: Number(mask >> 32n), eval: result.score ?? 0,
        }});
    } catch (error) {
        self.postMessage({ type, requestId, protocolVersion: 1, ok: false, error: String(error), payload: null });
    }
});
