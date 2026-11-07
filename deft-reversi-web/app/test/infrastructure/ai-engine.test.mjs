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

test('AiEngine rejects a pending search if the worker crashes', async () => {
    const originalWorker = globalThis.Worker;
    let worker;
    class FakeWorker {
        constructor() { this.listeners = {}; worker = this; }
        addEventListener(type, callback) { (this.listeners[type] ??= []).push(callback); }
        removeEventListener(type, callback) { this.listeners[type] = (this.listeners[type] ?? []).filter(c => c !== callback); }
        postMessage() {}
        terminate() { this.terminated = true; }
        emit(type, event) { for (const callback of this.listeners[type] ?? []) callback(event); }
    }
    globalThis.Worker = FakeWorker;
    try {
        let failure;
        const engine = new AiEngine();
        engine.initialize(() => {}, error => { failure = error; });
        worker.emit('message', { data: { type: 'ready', ok: true } });
        const search = engine.solveTurn(1n, 2n, 18);
        worker.emit('error', { message: 'worker crashed' });
        await assert.rejects(search, /Engine stopped/);
        assert.equal(failure.message, 'worker crashed');
        assert.equal(engine.isReady, false);
        assert.equal(worker.terminated, true);
    } finally { globalThis.Worker = originalWorker; }
});

test('AiEngine requests consent and reports cancellation without marking ready', async () => {
    const originalWorker = globalThis.Worker;
    let worker;
    class FakeWorker {
        constructor() { this.listeners = {}; this.messages = []; worker = this; }
        addEventListener(type, fn) { (this.listeners[type] ??= []).push(fn); }
        removeEventListener() {}
        postMessage(message) { this.messages.push(message); }
        terminate() {}
        emit(data) { for (const fn of this.listeners.message ?? []) fn({ data }); }
    }
    globalThis.Worker = FakeWorker;
    try {
        let cancelled = false;
        const engine = new AiEngine();
        engine.initialize(() => {}, () => {}, { onConsent: () => false, onCancel: () => { cancelled = true; } });
        worker.emit({ type: 'consent', info: { model: 'classic', bytes: 10 } });
        await new Promise(resolve => setImmediate(resolve));
        assert.deepEqual(worker.messages.at(-1), { type: 'consent-response', allowed: false });
        worker.emit({ type: 'ready', ok: false, cancelled: true });
        assert.equal(cancelled, true);
        assert.equal(engine.isReady, false);
        engine.terminate();
    } finally { globalThis.Worker = originalWorker; }
});
