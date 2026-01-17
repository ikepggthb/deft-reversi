import test from 'node:test';
import assert from 'node:assert/strict';
import { HistoryManager } from '../../application/history-manager.js';

test('HistoryManager: initial state', () => {
    const manager = new HistoryManager();
    assert.equal(manager.canUndo, false);
    assert.equal(manager.canRedo, false);
    assert.equal(manager.historyLength, 0);
    assert.equal(manager.futureLength, 0);
});

test('HistoryManager: push adds to history', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });

    assert.equal(manager.canUndo, true);
    assert.equal(manager.historyLength, 2);
});

test('HistoryManager: undo returns previous snapshot', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });

    const current = { value: 3 };
    const restored = manager.undo(current);

    assert.deepEqual(restored, { value: 2 });
    assert.equal(manager.historyLength, 1);
    assert.equal(manager.futureLength, 1);
    assert.equal(manager.canRedo, true);
});

test('HistoryManager: undo returns null when empty', () => {
    const manager = new HistoryManager();
    const result = manager.undo({ value: 1 });

    assert.equal(result, null);
    assert.equal(manager.futureLength, 0);
});

test('HistoryManager: redo returns future snapshot', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });

    const current1 = { value: 3 };
    manager.undo(current1); // history: [1], future: [3]

    const current2 = { value: 2 };
    const restored = manager.redo(current2);

    assert.deepEqual(restored, { value: 3 });
    assert.equal(manager.historyLength, 2);
    assert.equal(manager.futureLength, 0);
});

test('HistoryManager: redo returns null when empty', () => {
    const manager = new HistoryManager();
    const result = manager.redo({ value: 1 });

    assert.equal(result, null);
});

test('HistoryManager: clearFuture clears redo stack', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });
    manager.undo({ value: 3 });

    assert.equal(manager.canRedo, true);

    manager.clearFuture();

    assert.equal(manager.canRedo, false);
    assert.equal(manager.futureLength, 0);
});

test('HistoryManager: clear resets both stacks', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });
    manager.undo({ value: 3 });

    manager.clear();

    assert.equal(manager.canUndo, false);
    assert.equal(manager.canRedo, false);
    assert.equal(manager.historyLength, 0);
    assert.equal(manager.futureLength, 0);
});

test('HistoryManager: multiple undo/redo cycle', () => {
    const manager = new HistoryManager();
    manager.push({ value: 1 });
    manager.push({ value: 2 });
    manager.push({ value: 3 });

    // Undo twice
    // history: [1, 2, 3], future: []
    let current = { value: 4 };
    const r1 = manager.undo(current); // returns 3, future: [4]
    assert.deepEqual(r1, { value: 3 });
    // history: [1, 2], future: [4]

    current = { value: 3 };
    const r2 = manager.undo(current); // returns 2, future: [4, 3]
    assert.deepEqual(r2, { value: 2 });
    // history: [1], future: [4, 3]

    // Redo once (LIFO - pops from future stack)
    current = { value: 2 };
    const r3 = manager.redo(current); // returns 3 (last pushed to future), history adds 2
    assert.deepEqual(r3, { value: 3 });
    // history: [1, 2], future: [4]

    assert.equal(manager.historyLength, 2);
    assert.equal(manager.futureLength, 1);
});
