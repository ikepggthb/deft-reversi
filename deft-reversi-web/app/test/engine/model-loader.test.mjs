import test from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { loadModel, progressText } from '../../engine/model-loader.js';

function fixture() {
    const payload = new TextEncoder().encode('model data '.repeat(10000));
    const compressed = gzipSync(payload);
    return { payload, compressed, info: { file: 'eval-classic.dpr2.gz', compressedBytes: compressed.length,
        uncompressedBytes: payload.length, sha256: createHash('sha256').update(compressed).digest('hex') } };
}
function fakeCache() {
    const items = new Map();
    return { async match(key) { return items.get(key)?.clone(); }, async put(key, value) { items.set(key, value.clone()); }, async delete(key) { return items.delete(key); }, async keys() { return [...items.keys()].map(url => ({ url })); } };
}

test('asks before download, reports progress and reuses verified saved model', async () => {
    const originalFetch = globalThis.fetch, originalCaches = globalThis.caches;
    const { payload, compressed, info } = fixture();
    const cache = fakeCache();
    globalThis.caches = { open: async () => cache };
    let downloads = 0, confirmations = 0;
    globalThis.fetch = async url => String(url).includes('model-manifest.json')
        ? new Response(JSON.stringify({ classic: info }))
        : (downloads++, new Response(new ReadableStream({ start(controller) {
            controller.enqueue(compressed.subarray(0, 4)); controller.enqueue(compressed.subarray(4)); controller.close();
        } })));
    try {
        const seen = [];
        const result = await loadModel('classic', p => seen.push(p), async () => { confirmations++; return true; });
        assert.deepEqual(result, payload);
        assert.ok(seen.some(p => p.stage === 'download' && p.loaded === compressed.length));
        assert.ok(seen.some(p => p.stage === 'decompress' && p.loaded === payload.length));
        assert.equal(seen.at(-1).stage, 'initialize');
        assert.match(progressText(seen.find(p => p.stage === 'download')), /ダウンロード中/);
        assert.deepEqual(await loadModel('classic', () => {}, async () => { throw Error('unexpected prompt'); }), payload);
        assert.equal(downloads, 1);
        assert.equal(confirmations, 1);
    } finally { globalThis.fetch = originalFetch; globalThis.caches = originalCaches; }
});

test('cancel prevents model request; changes and failed storage require another consent', async () => {
    const originalFetch = globalThis.fetch, originalCaches = globalThis.caches;
    const { compressed, info } = fixture();
    let downloads = 0;
    globalThis.caches = { open: async () => ({ match: async () => undefined, put: async () => { throw Error('quota'); } }) };
    globalThis.fetch = async url => String(url).includes('model-manifest.json')
        ? new Response(JSON.stringify({ classic: info })) : (downloads++, new Response(compressed));
    try {
        await assert.rejects(loadModel('classic', () => {}, async () => false), { name: 'ModelDownloadCancelled' });
        assert.equal(downloads, 0);
        const events = [];
        await loadModel('classic', p => events.push(p), async () => true);
        assert.ok(events.some(p => p.stage === 'storage-warning'));
        await assert.rejects(loadModel('classic', () => {}, async () => false), { name: 'ModelDownloadCancelled' });
        assert.equal(downloads, 1);
        await assert.rejects(loadModel('unexpected'), /不明/);
    } finally { globalThis.fetch = originalFetch; globalThis.caches = originalCaches; }
});

test('offline reuse works and an updated model asks before downloading', async () => {
    const originalFetch = globalThis.fetch, originalCaches = globalThis.caches;
    const { payload, compressed, info } = fixture();
    const cache = fakeCache();
    globalThis.caches = { open: async () => cache };
    let offline = false, manifest = { classic: info }, downloads = 0;
    globalThis.fetch = async url => {
        if (offline) throw Error('offline');
        if (String(url).includes('model-manifest.json')) return new Response(JSON.stringify(manifest));
        downloads++;
        return new Response(compressed);
    };
    try {
        await loadModel('classic', () => {}, async () => true);
        offline = true;
        assert.deepEqual(await loadModel('classic', () => {}, async () => { throw Error('unexpected prompt'); }), payload);
        offline = false;
        manifest = { classic: { ...info, sha256: '0'.repeat(64) } };
        await assert.rejects(loadModel('classic', () => {}, async () => false), { name: 'ModelDownloadCancelled' });
        assert.equal(downloads, 1);
        await assert.rejects(loadModel('classic', () => {}, async () => true), /検証/);
    } finally { globalThis.fetch = originalFetch; globalThis.caches = originalCaches; }
});
