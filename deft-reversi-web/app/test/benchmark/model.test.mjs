import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compareAnswer, nodesPerSecond } from '../../benchmark/model.js';

test('FFO dataset has all numbered positions and reference answers', () => {
    const items = JSON.parse(readFileSync(new URL('../../benchmark/positions.json', import.meta.url)));
    assert.equal(items.length, 79);
    for (const [index, item] of items.entries()) {
        assert.equal(item.number, index + 1);
        assert.match(item.position, /^[XO-]{64} [XO]$/);
        if (item.number <= 49) assert.ok(item.answers.length);
        else assert.equal(item.answers.length, 0);
    }
});

test('compare only solved positions and accept tied optimal moves', () => {
    const answers = [{ move: 'A1', score: 4 }, { move: 'H8', score: 4 }, { move: 'B1', score: 2 }];
    assert.equal(compareAnswer({ solved: true, score: 4, bestMove: 'H8' }, answers), true);
    assert.equal(compareAnswer({ solved: true, score: 4, bestMove: 'B1' }, answers), false);
    assert.equal(compareAnswer({ solved: true, score: 2, bestMove: 'A1' }, answers), false);
    assert.equal(compareAnswer({ solved: false, score: 4, bestMove: 'A1' }, answers), null);
    assert.equal(compareAnswer({ solved: true, score: 4, bestMove: 'A1' }, []), null);
});

test('nodes per second is precise for large counts and zero duration', () => {
    assert.equal(nodesPerSecond('1000', 250), '4,000');
    assert.equal(nodesPerSecond('9007199254740993', 1000), '9,007,199,254,740,993');
    assert.equal(nodesPerSecond('0', 0), '—');
});
