// Requires Playwright and a local HTTP directory with baseline/, candidate/, model.gz.
// Example: node tools/bench/wasm_ffo40_49_browser.cjs http://127.0.0.1:8770/
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const root = process.argv[2];
if (!root) throw new Error('Specify the base URL of the benchmark directory');
const base = new URL(root.endsWith('/') ? root : `${root}/`);
const positions = JSON.parse(fs.readFileSync('deft-reversi-web/app/benchmark/positions.json'))
    .filter(item => item.number >= 40 && item.number <= 49);
if (positions.length !== 10) throw new Error('Expected ten FFO positions');
const source = fs.readFileSync(path.join(__dirname, 'wasm_ffo40_49_browser_worker.js'), 'utf8');
(async () => {
    const browser = await chromium.launch({
        ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
        args: ['--no-sandbox', '--disable-background-timer-throttling'],
    });
    try {
        const page = await browser.newPage();
        page.on('console', message => console.log(message.text()));
        await page.goto(base.href);
        let reference;
        for (const variant of ['baseline', 'candidate', 'candidate', 'baseline']) {
            const result = await page.evaluate(({ source, variant, positions, moduleUrl, modelUrl }) => new Promise((resolve, reject) => {
                const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
                const worker = new Worker(url, { type: 'module' });
                const cases = [];
                const cleanup = () => { worker.terminate(); URL.revokeObjectURL(url); };
                worker.onmessage = ({ data }) => {
                    if (data.type === 'error') { cleanup(); reject(new Error(data.message)); }
                    else if (data.type === 'position') {
                        cases.push([data.number, data.bestMove, data.score, data.nodes]);
                        console.log(JSON.stringify(data));
                    } else if (data.type === 'done') { cleanup(); resolve({ ...data, cases }); }
                };
                worker.onerror = event => { cleanup(); reject(new Error(event.message)); };
                worker.postMessage({ variant, positions, moduleUrl, modelUrl });
            }), { source, variant, positions,
                moduleUrl: new URL(`${variant}/deft_reversi_wasm_engine.js`, base).href,
                modelUrl: new URL('model.gz', base).href });
            const signature = JSON.stringify(result.cases);
            if (reference && signature !== reference) throw new Error('Search results or node counts differ');
            reference = signature;
            delete result.cases;
            console.log(JSON.stringify(result));
        }
    } finally {
        await browser.close();
    }
})().catch(error => { console.error(error); process.exitCode = 1; });
