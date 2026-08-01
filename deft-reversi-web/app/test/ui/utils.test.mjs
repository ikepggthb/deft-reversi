import test from 'node:test';
import assert from 'node:assert/strict';
import {
    clampInt,
    escapeHtml,
    formatRecordText,
    formatSigned,
    formatWinnerMessage,
    renderSparklineSvg,
} from '../../ui/utils.js';

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

// === formatRecordText ===

test('formatRecordText: omits pass by default', () => {
    assert.equal(formatRecordText(['f5', 'pass', 'd6']), 'f5 d6');
});

test('formatRecordText: can include pass when requested', () => {
    assert.equal(formatRecordText(['f5', 'pass', 'd6'], { includePass: true }), 'f5 pass d6');
});

test('formatRecordText: handles empty input', () => {
    assert.equal(formatRecordText([]), '');
});

// === formatWinnerMessage ===

test('formatWinnerMessage: renders black winner with display name', () => {
    assert.equal(formatWinnerMessage({ winner: 'black' }, 'あなた', 'AI Lv 4'), 'あなたの勝ち');
});

test('formatWinnerMessage: renders white winner with display name', () => {
    assert.equal(formatWinnerMessage({ winner: 'white' }, 'あなた', 'AI Lv 4'), 'AI Lv 4の勝ち');
});

test('formatWinnerMessage: renders draw', () => {
    assert.equal(formatWinnerMessage({ winner: null }, '黒', '白'), '引き分け');
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
