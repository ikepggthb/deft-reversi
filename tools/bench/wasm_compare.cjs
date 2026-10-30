// Compare two wasm-bindgen --target nodejs builds in alternating order.
// Usage: node tools/bench/wasm_compare.cjs /absolute/before.js /absolute/after.js
const fs = require('node:fs');
const zlib = require('node:zlib');
const path = require('node:path');
if (process.argv.length !== 4) throw new Error('Specify before.js and after.js');
const versions = process.argv.slice(2).map(p => require(path.resolve(p)));
const model = zlib.gunzipSync(fs.readFileSync('deft-reversi-web/app/assets/eval.dpr2.gz'));
const positions = fs.readFileSync('tools/bench/positions/ffo40-49.obf', 'utf8').trim().split('\n').slice(0, 5);
const times = [[], []];
let expected;
for (let round = 0; round < 8; round++) {
    for (const version of round % 2 ? [1, 0] : [0, 1]) {
        const { WasmEngine, WasmPosition } = versions[version];
        const engine = new WasmEngine(model, 32);
        const results = [];
        const start = performance.now();
        for (const input of positions) {
            const position = WasmPosition.fromString(input.trim());
            const result = engine.search(position, { goal: { kind: 'depth', value: 10 } });
            results.push([result.bestMove, result.score, result.nodes]);
            position.free();
        }
        times[version].push(Number((performance.now() - start).toFixed(1)));
        engine.free();
        const actual = JSON.stringify(results);
        if (expected && expected !== actual) throw new Error('Search results or node counts differ');
        expected = actual;
    }
}
console.log(JSON.stringify({ before_ms: times[0], after_ms: times[1], results: JSON.parse(expected) }, null, 2));
