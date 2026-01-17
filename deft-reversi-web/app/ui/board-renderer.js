/**
 * @fileoverview 盤面描画モジュール
 */

import { escapeHtml } from './utils.js';

const LETTERS = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];
const NUMBERS = ['1', '2', '3', '4', '5', '6', '7', '8'];

/**
 * @typedef {Object} BoardRenderOptions
 * @property {bigint} [legalMovesBits] 合法手のビットボード
 * @property {bigint} [blackBits] 黒石のビットボード
 * @property {bigint} [whiteBits] 白石のビットボード
 * @property {number | null} [lastMove] 最後の着手位置
 * @property {number | null} [nextOpeningPos] 定石の次の手の位置
 * @property {(number | null)[] | null} [scores] 各マスの評価値
 */

/**
 * 盤面をHTMLとして描画する
 * @param {BoardRenderOptions} options 描画オプション
 * @returns {string} HTMLマークアップ
 */
export function renderBoard(options = {}) {
    const legalBits = options.legalMovesBits ?? 0n;
    const blackBits = options.blackBits ?? 0n;
    const whiteBits = options.whiteBits ?? 0n;
    const lastMove = options.lastMove ?? null;
    const nextOpeningPos = options.nextOpeningPos ?? null;
    const scores = options.scores ?? null;

    // 最高スコアを計算（ハイライト用）
    let maxScore = null;
    if (scores) {
        for (let i = 0; i < 64; i++) {
            if (((legalBits >> BigInt(i)) & 1n) === 0n) continue;
            const v = scores[i];
            if (v === null || v === undefined) continue;
            if (maxScore === null || v > maxScore) maxScore = v;
        }
    }

    const tiles = [];
    for (let row = 0; row < 10; row++) {
        for (let col = 0; col < 10; col++) {
            tiles.push(renderCell(row, col, {
                legalBits,
                blackBits,
                whiteBits,
                lastMove,
                nextOpeningPos,
                scores,
                maxScore,
            }));
        }
    }

    return `<div class="board-shell" role="grid">${tiles.join('')}</div>`;
}

/**
 * 個別のセルを描画
 * @private
 */
function renderCell(row, col, ctx) {
    // 四隅
    if ((row === 0 || row === 9) && (col === 0 || col === 9)) {
        return `<div class="coord"></div>`;
    }

    // 上下の列ラベル（A-H）
    if ((row === 0 || row === 9) && col >= 1 && col <= 8) {
        return `<div class="coord">${LETTERS[col - 1]}</div>`;
    }

    // 左右の行ラベル（1-8）
    if ((col === 0 || col === 9) && row >= 1 && row <= 8) {
        return `<div class="coord">${NUMBERS[row - 1]}</div>`;
    }

    // 盤面内のセル
    const x = col - 1;
    const y = row - 1;
    const pos = y * 8 + x;

    const isLegal = ((ctx.legalBits >> BigInt(pos)) & 1n) === 1n;
    const isBlack = ((ctx.blackBits >> BigInt(pos)) & 1n) === 1n;
    const isWhite = ((ctx.whiteBits >> BigInt(pos)) & 1n) === 1n;
    const isLast = ctx.lastMove === pos;
    const isNextOpening = ctx.nextOpeningPos === pos;
    const score = ctx.scores ? ctx.scores[pos] : null;
    const showScore = score !== null && score !== undefined && isLegal;
    const scoreColor = ctx.maxScore !== null && score === ctx.maxScore ? 'style="color: var(--score-best)"' : '';

    const classes = [
        'cell',
        isLegal ? 'legal' : '',
        isLast ? 'last-move' : '',
        isNextOpening ? 'next-opening' : '',
    ]
        .filter(Boolean)
        .join(' ');

    const stone = isBlack
        ? `<div class="stone black"></div>`
        : isWhite
            ? `<div class="stone white"></div>`
            : '';
    const scoreHtml = showScore ? `<div class="score" ${scoreColor}>${escapeHtml(String(score))}</div>` : '';

    return `<button class="${classes}" data-click="cell" data-pos="${pos}" aria-label="cell ${pos}">${stone}${scoreHtml}</button>`;
}

/**
 * ViewModelから盤面を描画する（UIクラスとの互換性用）
 * @param {import('../application/game-service.js').GameViewModel | undefined} vm
 * @returns {string} HTMLマークアップ
 */
export function renderBoardFromViewModel(vm) {
    return renderBoard({
        legalMovesBits: vm?.legalMovesBits,
        blackBits: vm?.blackBits,
        whiteBits: vm?.whiteBits,
        lastMove: vm?.lastMove,
        nextOpeningPos: vm?.humanOpeningNextPosition,
        scores: vm?.eval,
    });
}

/**
 * Reversi Pro スタイルの盤面を描画
 * @param {import('../application/game-service.js').GameViewModel | undefined} vm
 * @returns {string} HTMLマークアップ
 */
export function renderRpBoard(vm) {
    const legalBits = vm?.legalMovesBits ?? 0n;
    const blackBits = vm?.blackBits ?? 0n;
    const whiteBits = vm?.whiteBits ?? 0n;
    const lastMove = vm?.lastMove ?? null;

    // Column labels
    const coordRow = `
        <div class="rp-coord-row">
            ${LETTERS.map(l => `<div class="rp-coord-label">${l}</div>`).join('')}
        </div>
    `;

    // Board cells
    const cells = [];
    for (let y = 0; y < 8; y++) {
        for (let x = 0; x < 8; x++) {
            const pos = y * 8 + x;
            const isLegal = ((legalBits >> BigInt(pos)) & 1n) === 1n;
            const isBlack = ((blackBits >> BigInt(pos)) & 1n) === 1n;
            const isWhite = ((whiteBits >> BigInt(pos)) & 1n) === 1n;
            const isLast = lastMove === pos;

            const classes = [
                'rp-cell',
                isLegal ? 'legal' : '',
                isLast ? 'last-move' : '',
            ].filter(Boolean).join(' ');

            let stone = '';
            if (isBlack) {
                stone = '<div class="rp-stone black"></div>';
            } else if (isWhite) {
                stone = '<div class="rp-stone white"></div>';
            }

            cells.push(`<button class="${classes}" data-click="cell" data-pos="${pos}" aria-label="cell ${pos}">${stone}</button>`);
        }
    }

    return `
        <div class="rp-board-container">
            <div class="rp-board-inner">
                ${coordRow}
                <div class="rp-board-grid" role="grid">
                    ${cells.join('')}
                </div>
            </div>
        </div>
    `;
}
