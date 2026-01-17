/**
 * @fileoverview 分析ペインの描画モジュール
 */

import { escapeHtml, formatSigned, computeMaterialDiffSeries, renderSparklineSvg } from './utils.js';

/**
 * @typedef {Object} AnalysisEntry
 * @property {number} ply
 * @property {import('../domain/types.js').Turn} nextTurn
 * @property {number} evalToMove
 * @property {number} evalBlack
 * @property {string | null} bestMove
 */

/**
 * @typedef {Object} AnalysisMoveListOptions
 * @property {string[]} record 棋譜
 * @property {AnalysisEntry[] | null} analysis 分析結果
 * @property {number} cursor 現在のカーソル位置
 */

/**
 * 分析手順リストを描画
 * @param {AnalysisMoveListOptions} options
 * @returns {string} HTMLマークアップ
 */
export function renderAnalysisMoveList(options) {
    const { record, analysis, cursor } = options;

    const items = record
        .map((m, i) => {
            const plyAfter = i + 1;
            const n = i + 1;
            const evalAfter = analysis && analysis[plyAfter] ? analysis[plyAfter].evalBlack : null;
            const bestBefore = analysis && analysis[i] ? analysis[i].bestMove : null;
            const isBest = bestBefore && String(m).toLowerCase() === String(bestBefore).toLowerCase();
            const active = plyAfter === cursor;
            return `<button class="secondary-btn" style="text-align:left; padding:10px 12px; ${
                active ? 'border-color: rgba(16,185,129,0.35);' : ''
            }" data-click="seek-ply" data-ply="${plyAfter}">
                ${n}. ${escapeHtml(String(m))}${evalAfter !== null ? ` <span style="color:var(--muted)">(${formatSigned(evalAfter)})</span>` : ''}
                ${bestBefore ? ` <span style="color:var(--muted); font-size:12px;">best: ${escapeHtml(String(bestBefore))}</span>` : ''}
                ${isBest ? ` <span style="color:rgba(16,185,129,0.95)">✓</span>` : ''}
            </button>`;
        })
        .join('');

    return `<div class="section">
        <h2>MOVE LIST</h2>
        <div class="mini">cursor: ${cursor}/${record.length}</div>
        <div style="margin-top:10px; display:flex; flex-direction:column; gap:8px; max-height: 240px; overflow:auto;">
          ${items || `<div style="color:var(--muted); font-size:12px;">No moves</div>`}
        </div>
    </div>`;
}

/**
 * @typedef {Object} AnalysisGraphOptions
 * @property {string[]} record 棋譜
 * @property {AnalysisEntry[] | null} analysis 分析結果
 * @property {number} cursor カーソル位置
 * @property {number | null} hover ホバー位置
 */

/**
 * 分析グラフを描画
 * @param {AnalysisGraphOptions} options
 * @returns {string} HTMLマークアップ
 */
export function renderAnalysisGraph(options) {
    const { record, analysis, cursor, hover } = options;

    const series = analysis
        ? analysis.map((x) => x.evalBlack)
        : computeMaterialDiffSeries(record);

    const cursorEval =
        series.length > 0 && cursor >= 0 && cursor < series.length ? series[cursor] : series.at(-1) ?? 0;
    const hoverEval =
        hover !== null && hover >= 0 && hover < series.length ? series[hover] : null;

    const svg = renderSparklineSvg(series, { cursor, hover });

    return `<div class="section">
        <h2>GRAPH</h2>
        <div class="graph">${svg}</div>
        <div style="margin-top:10px;" class="mini">
          cursor: ply ${cursor}/${Math.max(0, record.length)} eval ${formatSigned(cursorEval)}
          ${hoverEval !== null ? ` / hover: ply ${hover} eval ${formatSigned(hoverEval)}` : ''}
        </div>
    </div>`;
}
