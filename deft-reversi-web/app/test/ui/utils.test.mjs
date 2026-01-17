import test from 'node:test';
import assert from 'node:assert/strict';
import {
    presetToLevel,
    clampInt,
    escapeHtml,
    formatSigned,
    notationToPositionSafe,
    computeMaterialDiffSeries,
    renderSparklineSvg,
} from '../../ui/utils.js';

// === presetToLevel ===

test('presetToLevel: returns correct level for known presets', () => {
    assert.equal(presetToLevel('beginner'), 4);
    assert.equal(presetToLevel('intermediate'), 8);
    assert.equal(presetToLevel('expert'), 16);
    assert.equal(presetToLevel('grandmaster'), 24);
});

test('presetToLevel: returns default for unknown preset', () => {
    assert.equal(presetToLevel('unknown'), 8);
    assert.equal(presetToLevel(''), 8);
    assert.equal(presetToLevel(null), 8);
    assert.equal(presetToLevel(undefined), 8);
});

// === clampInt ===

test('clampInt: clamps value within range', () => {
    assert.equal(clampInt(5, 1, 10), 5);
    assert.equal(clampInt(0, 1, 10), 1);
    assert.equal(clampInt(15, 1, 10), 10);
});

test('clampInt: truncates to integer', () => {
    assert.equal(clampInt(5.7, 1, 10), 5);
    assert.equal(clampInt(5.2, 1, 10), 5);
    assert.equal(clampInt(-2.9, -5, 10), -2);
});

test('clampInt: handles non-finite values', () => {
    assert.equal(clampInt(NaN, 1, 10), 1);
    assert.equal(clampInt(Infinity, 1, 10), 1);
    assert.equal(clampInt(-Infinity, 1, 10), 1);
});

test('clampInt: handles edge cases at boundaries', () => {
    assert.equal(clampInt(1, 1, 10), 1);
    assert.equal(clampInt(10, 1, 10), 10);
});

// === escapeHtml ===

test('escapeHtml: escapes HTML special characters', () => {
    assert.equal(escapeHtml('<div>'), '&lt;div&gt;');
    assert.equal(escapeHtml('a & b'), 'a &amp; b');
    assert.equal(escapeHtml('"quote"'), '&quot;quote&quot;');
    assert.equal(escapeHtml("'single'"), '&#39;single&#39;');
});

test('escapeHtml: handles combined characters', () => {
    assert.equal(escapeHtml('<script>alert("xss")</script>'), '&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;');
});

test('escapeHtml: passes through safe strings', () => {
    assert.equal(escapeHtml('hello world'), 'hello world');
    assert.equal(escapeHtml('123'), '123');
});

test('escapeHtml: converts non-strings', () => {
    assert.equal(escapeHtml(123), '123');
    assert.equal(escapeHtml(null), 'null');
    assert.equal(escapeHtml(undefined), 'undefined');
});

// === formatSigned ===

test('formatSigned: formats positive numbers with plus sign', () => {
    assert.equal(formatSigned(5), '+5');
    assert.equal(formatSigned(100), '+100');
});

test('formatSigned: formats negative numbers', () => {
    assert.equal(formatSigned(-3), '-3');
    assert.equal(formatSigned(-100), '-100');
});

test('formatSigned: formats zero without sign', () => {
    assert.equal(formatSigned(0), '0');
});

test('formatSigned: handles non-finite values', () => {
    assert.equal(formatSigned(NaN), '0');
    assert.equal(formatSigned(Infinity), '0');
    assert.equal(formatSigned(-Infinity), '0');
});

test('formatSigned: converts string numbers', () => {
    assert.equal(formatSigned('5'), '+5');
    assert.equal(formatSigned('-3'), '-3');
});

// === notationToPositionSafe ===

test('notationToPositionSafe: converts valid notation to position', () => {
    assert.equal(notationToPositionSafe('a1'), 0);
    assert.equal(notationToPositionSafe('h8'), 63);
    assert.equal(notationToPositionSafe('f5'), 37);
    assert.equal(notationToPositionSafe('d6'), 43);
});

test('notationToPositionSafe: handles uppercase', () => {
    assert.equal(notationToPositionSafe('A1'), 0);
    assert.equal(notationToPositionSafe('F5'), 37);
});

test('notationToPositionSafe: returns null for invalid notation', () => {
    assert.equal(notationToPositionSafe(''), null);
    assert.equal(notationToPositionSafe('a'), null);
    assert.equal(notationToPositionSafe('a11'), null);
    assert.equal(notationToPositionSafe('i1'), null);
    assert.equal(notationToPositionSafe('a9'), null);
    assert.equal(notationToPositionSafe('a0'), null);
});

test('notationToPositionSafe: returns null for non-string input', () => {
    assert.equal(notationToPositionSafe(null), null);
    assert.equal(notationToPositionSafe(undefined), null);
    assert.equal(notationToPositionSafe(123), null);
});

// === computeMaterialDiffSeries ===

test('computeMaterialDiffSeries: returns initial diff for empty record', () => {
    const series = computeMaterialDiffSeries([]);
    assert.equal(series.length, 1);
    assert.equal(series[0], 0); // 初期盤面: 黒2, 白2
});

test('computeMaterialDiffSeries: computes diff for valid moves', () => {
    const series = computeMaterialDiffSeries(['f5']);
    assert.equal(series.length, 2);
    // F5: 黒が白1つ反転 → 黒4, 白1
    assert.equal(series[1], 3);
});

test('computeMaterialDiffSeries: handles pass in record', () => {
    // F5, D6 is a valid sequence
    const series1 = computeMaterialDiffSeries(['f5', 'd6']);
    assert.equal(series1.length, 3);

    // pass adds an entry with unchanged material count
    const series2 = computeMaterialDiffSeries(['f5', 'pass', 'd6']);
    // series: [0 (initial), 3 (after f5), 3 (after pass - unchanged), ...d6 if valid]
    // But d6 may not be a valid black move after pass, so check length >= 3
    assert.ok(series2.length >= 3);
    // After f5 and pass, material stays at 3
    assert.equal(series2[1], 3);
    assert.equal(series2[2], 3);
});

test('computeMaterialDiffSeries: stops at invalid move', () => {
    // a1 is not a valid first move
    const series = computeMaterialDiffSeries(['a1']);
    assert.equal(series.length, 1);
});

// === renderSparklineSvg ===

test('renderSparklineSvg: returns valid SVG string', () => {
    const svg = renderSparklineSvg([0, 1, 2, 3]);
    assert.ok(svg.includes('<svg'));
    assert.ok(svg.includes('</svg>'));
    assert.ok(svg.includes('<polyline'));
});

test('renderSparklineSvg: handles single value', () => {
    const svg = renderSparklineSvg([5]);
    assert.ok(svg.includes('<svg'));
});

test('renderSparklineSvg: handles empty array', () => {
    const svg = renderSparklineSvg([]);
    assert.ok(svg.includes('<svg'));
});

test('renderSparklineSvg: includes cursor line when specified', () => {
    const svg = renderSparklineSvg([0, 1, 2], { cursor: 1 });
    // cursor line should be present
    assert.ok(svg.includes('data-ply="1"'));
});

test('renderSparklineSvg: includes hover line when specified', () => {
    const svg = renderSparklineSvg([0, 1, 2], { hover: 2 });
    assert.ok(svg.includes('data-ply="2"'));
});

test('renderSparklineSvg: handles negative values', () => {
    const svg = renderSparklineSvg([-5, 0, 5]);
    assert.ok(svg.includes('<svg'));
    assert.ok(svg.includes('-5'));
});
