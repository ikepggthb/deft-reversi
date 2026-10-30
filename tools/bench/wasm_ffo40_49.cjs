// Solve FFO 40–49 exactly, reporting per-position and summed search time.
// Usage: node tools/bench/wasm_ffo40_49.cjs /absolute/path/to/wasm-bindgen-node.js
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');
if (process.argv.length !== 3) throw new Error('Specify a wasm-bindgen nodejs module');
const { WasmEngine, WasmPosition } = require(path.resolve(process.argv[2]));
const model = zlib.gunzipSync(fs.readFileSync('deft-reversi-web/app/assets/eval.dpr2.gz'));
const positions = JSON.parse(fs.readFileSync('deft-reversi-web/app/benchmark/positions.json')).filter(p => p.number >= 40 && p.number <= 49);
if (positions.length !== 10) throw new Error('Expected ten FFO positions');
const engine = new WasmEngine(model, 32);
let totalMs = 0, totalNodes = 0n;
for (const item of positions) {
    const position = WasmPosition.fromString(item.position);
    engine.clearTt();
    const result = engine.search(position, { goal: { kind: 'endgame' } });
    position.free();
    const score = Math.max(...item.answers.map(a => a.score));
    const valid = result.solved && result.stopReason === 'Completed' && result.score === score && item.answers.some(a => a.move === result.bestMove && a.score === score);
    if (!valid) throw new Error(`FFO ${item.number}: unexpected or incomplete result ${JSON.stringify(result)}`);
    totalMs += result.elapsedMs;
    totalNodes += BigInt(result.nodes);
    console.log(JSON.stringify({ number: item.number, bestMove: result.bestMove, score: result.score, nodes: result.nodes, elapsedMs: result.elapsedMs }));
}
engine.free();
console.log(JSON.stringify({ totalMs, totalNodes: totalNodes.toString() }));
