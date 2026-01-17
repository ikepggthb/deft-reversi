/**
 * @fileoverview UI層で使用するユーティリティ関数
 */

import { Board } from '../domain/board.js';
import { PRESET_DEPTHS, AI_LEVEL_DEFAULT } from '../config/constants.js';

/**
 * プリセット名からAIレベルを取得
 * @param {string} key プリセット名
 * @returns {number} AIレベル
 */
export function presetToLevel(key) {
    return PRESET_DEPTHS[key] ?? AI_LEVEL_DEFAULT;
}

/**
 * 数値を整数に変換し、範囲内に収める
 * @param {number} n 変換する値
 * @param {number} min 最小値
 * @param {number} max 最大値
 * @returns {number} 範囲内の整数
 */
export function clampInt(n, min, max) {
    const x = Number.isFinite(n) ? Math.trunc(n) : min;
    return Math.min(max, Math.max(min, x));
}

/**
 * HTML特殊文字をエスケープ
 * @param {string} s エスケープする文字列
 * @returns {string} エスケープされた文字列
 */
export function escapeHtml(s) {
    return String(s)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

/**
 * 数値を符号付き文字列に変換
 * @param {number} n 変換する数値
 * @returns {string} 符号付き文字列（例: "+5", "-3", "0"）
 */
export function formatSigned(n) {
    const x = Number(n);
    if (!Number.isFinite(x)) return '0';
    return x > 0 ? `+${x}` : `${x}`;
}

/**
 * 棋譜表記を盤面位置に変換（安全版）
 * @param {string} notation 棋譜表記（例: "f5"）
 * @returns {number | null} 位置（0-63）、無効な場合はnull
 */
export function notationToPositionSafe(notation) {
    if (typeof notation !== 'string' || notation.length !== 2) return null;
    const letters = 'abcdefgh';
    const numbers = '12345678';
    const col = letters.indexOf(notation[0].toLowerCase());
    const row = numbers.indexOf(notation[1]);
    if (col === -1 || row === -1) return null;
    return row * 8 + col;
}

/**
 * 石数差（black - white）を手順ごとに計算する
 * @param {ReadonlyArray<string>} record 棋譜（例: ["f5","d6","pass",...]）
 * @returns {number[]} 各手順での石数差
 */
export function computeMaterialDiffSeries(record) {
    try {
        let board = Board.initial();
        /** @type {number[]} */
        const series = [];
        series.push(board.blackCount - board.whiteCount);
        for (const m of record) {
            const t = String(m).toLowerCase();
            if (t === 'pass') {
                board = board.applyPass();
            } else {
                const pos = notationToPositionSafe(t);
                if (pos === null) break;
                if (board.canPlace(pos)) {
                    board = board.applyMove(pos);
                } else if (board.mustPass()) {
                    board = board.applyPass();
                    if (board.canPlace(pos)) {
                        board = board.applyMove(pos);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            series.push(board.blackCount - board.whiteCount);
        }
        return series;
    } catch (error) {
        console.warn('computeMaterialDiffSeries: failed to compute series', error);
        return [0];
    }
}

// === SVG描画関連 ===

const SVG_WIDTH = 320;
const SVG_HEIGHT = 100;
const SVG_PADDING = 8;

/**
 * 評価値の推移をスパークラインSVGで描画
 * @param {number[]} series 評価値の配列
 * @param {{ cursor?: number, hover?: number }} [options] オプション
 * @returns {string} SVGマークアップ
 */
export function renderSparklineSvg(series, options = {}) {
    const w = SVG_WIDTH;
    const h = SVG_HEIGHT;
    const pad = SVG_PADDING;
    const n = Math.max(1, series.length);
    const cursor = Number.isInteger(options.cursor) ? options.cursor : null;
    const hover = Number.isInteger(options.hover) ? options.hover : null;

    let min = Infinity;
    let max = -Infinity;
    for (const v of series) {
        if (v < min) min = v;
        if (v > max) max = v;
    }
    if (min === max) {
        min -= 1;
        max += 1;
    }

    const toX = (i) => pad + (i * (w - pad * 2)) / Math.max(1, n - 1);
    const toY = (v) => {
        const t = (v - min) / (max - min);
        return pad + (1 - t) * (h - pad * 2);
    };

    const points = series.map((v, i) => `${toX(i).toFixed(1)},${toY(v).toFixed(1)}`).join(' ');
    const zeroY = toY(0);

    const grid = [max, 0, min]
        .map((v) => {
            const y = toY(v);
            const label = v === 0 ? '0' : String(v);
            return `
              <line x1="${pad}" y1="${y.toFixed(1)}" x2="${(w - pad).toFixed(1)}" y2="${y.toFixed(1)}" stroke="var(--line)" stroke-width="1"/>
              <text x="${pad.toFixed(1)}" y="${(y - 4).toFixed(1)}" fill="var(--muted)" font-size="10">${escapeHtml(label)}</text>
            `.trim();
        })
        .join('');

    const cursorLine = (() => {
        if (cursor === null || cursor < 0 || cursor >= n) return '';
        const x = toX(cursor);
        return `<line x1="${x.toFixed(1)}" y1="${pad}" x2="${x.toFixed(1)}" y2="${(h - pad).toFixed(1)}" stroke="rgba(15,23,42,0.24)" stroke-width="1" />`;
    })();

    const hoverLine = (() => {
        if (hover === null || hover < 0 || hover >= n) return '';
        const x = toX(hover);
        return `<line x1="${x.toFixed(1)}" y1="${pad}" x2="${x.toFixed(1)}" y2="${(h - pad).toFixed(1)}" stroke="rgba(16,185,129,0.25)" stroke-width="1" />`;
    })();

    const dots = series
        .map((v, i) => {
            const cx = toX(i);
            const cy = toY(v);
            const active = cursor === i;
            const hovered = hover === i;
            const r = active ? 5 : hovered ? 4.5 : 3.5;
            const fill = active ? 'var(--text)' : 'var(--accent)';
            return `<circle data-ply="${i}" cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="${r}" fill="${fill}" stroke="rgba(15,23,42,0.28)" stroke-width="1" />`;
        })
        .join('');

    return `
      <svg viewBox="0 0 ${w} ${h}" width="100%" height="100%" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="graph">
        ${grid}
        <line x1="${pad}" y1="${zeroY.toFixed(1)}" x2="${w - pad}" y2="${zeroY.toFixed(1)}" stroke="rgba(15,23,42,0.18)" stroke-width="1"/>
        ${hoverLine}
        ${cursorLine}
        <polyline points="${points}" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
        ${dots}
      </svg>
    `.trim();
}
