import init, { WasmEngine, WasmPosition } from '../pkg/deft_reversi_wasm.js';

async function initialize() {
    await init();
    const response = await fetch(new URL('../assets/deft_eval_2024-01-27.json.gz', import.meta.url));
    if (!response.ok) throw new Error(`評価データの読み込みに失敗しました: ${response.status}`);
    const stream = response.body.pipeThrough(new DecompressionStream('gzip'));
    const legacy = await new Response(stream).json();
    const phases = Array.from({ length: 60 }, (_, n) => {
        const phase = legacy.eval[n % 2][Math.floor(n / 2)];
        return { pattern_weights: phase.pattern_eval, mobility_weights: phase.mobility_eval, bias: phase.const_eval };
    });
    const bytes = new TextEncoder().encode(JSON.stringify({ evaluator: { phases } }));
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
        const board = Array.from({ length: 64 }, (_, i) => {
            const bit = 1n << BigInt(i);
            return player & bit ? 'X' : opponent & bit ? 'O' : '-';
        }).join('');
        const position = WasmPosition.fromString(`${board} X`);
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
