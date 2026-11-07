import test from 'node:test';
import assert from 'node:assert/strict';
import { AiEngine } from '../../infrastructure/ai-engine.js';

test('AiEngine passes selected model, reports progress and rejects requests when replaced', async () => {
    const originalWorker = globalThis.Worker;
    const workers = [];
    class FakeWorker {
        constructor() { this.listeners = {}; this.messages = []; workers.push(this); }
        addEventListener(type, callback) { (this.listeners[type] ??= []).push(callback); }
        removeEventListener(type, callback) { this.listeners[type] = (this.listeners[type] ?? []).filter(c => c !== callback); }
        postMessage(message) { this.messages.push(message); }
        terminate() { this.terminated = true; }
        emit(type, data) { for (const callback of this.listeners[type] ?? []) callback({ data }); }
    }
    globalThis.Worker = FakeWorker;
    try {
        const engine = new AiEngine();
        const progress = [];
        engine.initialize(() => {}, () => {}, { model: 'pen', onProgress: p => progress.push(p) });
        assert.deepEqual(workers[0].messages[0], { type: 'init', model: 'pen' });
        workers[0].emit('message', { type: 'progress', progress: { stage: 'download', loaded: 5, total: 10 } });
        assert.equal(progress[0].loaded, 5);
        workers[0].emit('message', { type: 'ready', ok: true });
        const search = engine.solveTurn(1n, 2n, 1);
        engine.initialize(() => {}, () => {}, { model: 'classic' });
        await assert.rejects(search, /Engine stopped/);
        assert.equal(workers[0].terminated, true);
        assert.deepEqual(workers[1].messages[0], { type: 'init', model: 'classic' });
        assert.equal(engine.isReady, false);
        engine.terminate();
    } finally { globalThis.Worker = originalWorker; }
});
