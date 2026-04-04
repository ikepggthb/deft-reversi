/**
 * @fileoverview UI層で使用するユーティリティ関数
 */


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
 * 棋譜表示用に文字列化する
 * @param {ReadonlyArray<string>} record
 * @param {{ includePass?: boolean }} [options]
 * @returns {string}
 */
export function formatRecordText(record, options = {}) {
    if (!Array.isArray(record) || record.length === 0) return '';
    const includePass = options.includePass ?? false;
    return record
        .filter((move) => includePass || String(move).toLowerCase() !== 'pass')
        .join(' ');
}

/**
 * 勝敗表示文言を生成する
 * @param {{ winner: string | null }} result
 * @param {string} blackName
 * @param {string} whiteName
 * @returns {string}
 */
export function formatWinnerMessage(result, blackName, whiteName) {
    if (!result?.winner) return '引き分け';
    return result.winner === 'black' ? `${blackName}の勝ち` : `${whiteName}の勝ち`;
}

// === SVG描画関連 ===

const SVG_WIDTH = 320;
const SVG_HEIGHT = 100;
const SVG_PADDING = 8;

/**
 * 評価値の推移をスパークラインSVGで描画
 * @param {number[]} series 評価値の配列
 * @param {{ cursor?: number, hover?: number | null, overlaySeries?: number[] | null, overlayStart?: number, overlayCursor?: number, overlayHover?: number | null, marker?: number }} [options] オプション
 * @returns {string} SVGマークアップ
 */
export function renderSparklineSvg(series, options = {}) {
    const w = SVG_WIDTH;
    const h = SVG_HEIGHT;
    const pad = SVG_PADDING;
    const overlaySeries = Array.isArray(options.overlaySeries) && options.overlaySeries.length > 0 ? options.overlaySeries : null;
    const overlayStart = Number.isInteger(options.overlayStart) ? Math.max(0, Number(options.overlayStart)) : 0;
    const overlayEnd = overlaySeries ? overlayStart + overlaySeries.length : 0;
    const n = Math.max(1, series.length, overlayEnd);
    const cursor = Number.isInteger(options.cursor) ? options.cursor : null;
    const hover = Number.isInteger(options.hover) ? options.hover : null;
    const overlayCursor = Number.isInteger(options.overlayCursor) ? options.overlayCursor : null;
    const overlayHover = Number.isInteger(options.overlayHover) ? options.overlayHover : null;
    const marker = Number.isInteger(options.marker) ? options.marker : null;

    let min = Infinity;
    let max = -Infinity;
    for (const v of [...series, ...(overlaySeries ?? [])]) {
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
    const overlayPoints = overlaySeries
        ? overlaySeries.map((v, i) => `${toX(i + overlayStart).toFixed(1)},${toY(v).toFixed(1)}`).join(' ')
        : '';
    const zeroY = toY(0);

    const grid = [max, 0, min]
        .map((v) => {
            const y = toY(v);
            const label = v === 0 ? '0' : String(v);
            const labelY = Math.max(10, y - 4);
            return `
              <line x1="${pad}" y1="${y.toFixed(1)}" x2="${(w - pad).toFixed(1)}" y2="${y.toFixed(1)}" stroke="var(--line)" stroke-width="1"/>
              <text x="${pad.toFixed(1)}" y="${labelY.toFixed(1)}" fill="var(--muted)" font-size="10">${escapeHtml(label)}</text>
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
    const overlayCursorLine = (() => {
        if (overlayCursor === null || !overlaySeries || overlayCursor < overlayStart || overlayCursor >= overlayEnd) return '';
        const x = toX(overlayCursor);
        return `<line x1="${x.toFixed(1)}" y1="${pad}" x2="${x.toFixed(1)}" y2="${(h - pad).toFixed(1)}" stroke="rgba(249,115,22,0.28)" stroke-width="1" />`;
    })();
    const markerLine = (() => {
        if (marker === null || marker < 0 || marker >= n) return '';
        const x = toX(marker);
        return `<line x1="${x.toFixed(1)}" y1="${pad}" x2="${x.toFixed(1)}" y2="${(h - pad).toFixed(1)}" stroke="rgba(249,115,22,0.45)" stroke-dasharray="3 3" stroke-width="1.2" />`;
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
    const overlayDots = overlaySeries
        ? overlaySeries
              .map((v, i) => {
                  const xIndex = i + overlayStart;
                  const cx = toX(xIndex);
                  const cy = toY(v);
                  const active = overlayCursor === xIndex;
                  const hovered = overlayHover === xIndex;
                  const r = active ? 4.8 : hovered ? 4.3 : 3.2;
                  const fill = active ? '#c2410c' : '#fb923c';
                  return `<circle cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="${r}" fill="${fill}" stroke="rgba(124,45,18,0.35)" stroke-width="1" />`;
              })
              .join('')
        : '';

    return `
      <svg viewBox="0 0 ${w} ${h}" width="100%" height="100%" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="graph">
        ${grid}
        <line x1="${pad}" y1="${zeroY.toFixed(1)}" x2="${w - pad}" y2="${zeroY.toFixed(1)}" stroke="rgba(15,23,42,0.18)" stroke-width="1"/>
        ${hoverLine}
        ${cursorLine}
        ${markerLine}
        ${overlayCursorLine}
        <polyline points="${points}" fill="none" stroke="var(--accent)" stroke-width="${overlaySeries ? '1.6' : '2'}" stroke-linecap="round" stroke-linejoin="round"/>
        ${overlaySeries ? `<polyline points="${overlayPoints}" fill="none" stroke="#f97316" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>` : ''}
        ${dots}
        ${overlayDots}
      </svg>
    `.trim();
}
