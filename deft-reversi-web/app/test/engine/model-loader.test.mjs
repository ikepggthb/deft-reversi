import test from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync } from 'node:zlib';
import { loadModel, progressText } from '../../engine/model-loader.js';

test('loadModel reports download and decompression progress using manifest sizes', async () => {
    const originalFetch = globalThis.fetch;
    const payload = new TextEncoder().encode('model data '.repeat(10000));
    const compressed = gzipSync(payload);
    const manifest = { classic: { file: 'eval-classic.dpr2.gz', compressedBytes: compressed.length, uncompressedBytes: payload.length } };
    globalThis.fetch = async url => String(url).endsWith('model-manifest.json')
        ? new Response(JSON.stringify(manifest))
        : new Response(new ReadableStream({ start(controller) {
            controller.enqueue(compressed.subarray(0, 4));
            controller.enqueue(compressed.subarray(4));
            controller.close();
        } }));
    try {
        const seen = [];
        const result = await loadModel('classic', p => seen.push(p));
        assert.deepEqual(result, payload);
        assert.ok(seen.some(p => p.stage === 'download' && p.loaded === compressed.length && p.total === compressed.length));
        assert.ok(seen.some(p => p.stage === 'decompress' && p.loaded === payload.length && p.total === payload.length));
        assert.equal(seen.at(-1).stage, 'initialize');
        assert.match(progressText(seen.find(p => p.stage === 'download')), /ダウンロード中/);
    } finally { globalThis.fetch = originalFetch; }
});

test('loadModel rejects unknown models and truncated downloads', async () => {
    await assert.rejects(loadModel('unexpected'), /不明/);
    const originalFetch = globalThis.fetch;
    globalThis.fetch = async url => String(url).endsWith('model-manifest.json')
        ? new Response(JSON.stringify({ pen: { file: 'eval-pen.dpr2.gz', compressedBytes: 10, uncompressedBytes: 20 } }))
        : new Response(new Uint8Array([1, 2, 3]));
    try { await assert.rejects(loadModel('pen'), /途中で終了/); }
    finally { globalThis.fetch = originalFetch; }
});
