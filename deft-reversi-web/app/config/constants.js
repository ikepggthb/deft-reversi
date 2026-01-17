/**
 * @fileoverview アプリケーション全体で使用する定数
 */

// === AI関連 ===

/** AIレベルの最小値 */
export const AI_LEVEL_MIN = 1;

/** AIレベルの最大値 */
export const AI_LEVEL_MAX = 24;

/** AIレベルのデフォルト値 */
export const AI_LEVEL_DEFAULT = 8;

/** AIエンジンWorkerのタイムアウト（ミリ秒） */
export const AI_ENGINE_TIMEOUT_MS = 120000;

// === プリセット難易度 ===

/** @type {Readonly<Record<string, number>>} */
export const PRESET_DEPTHS = Object.freeze({
    beginner: 4,
    intermediate: 8,
    expert: 16,
    grandmaster: 24,
});

// === UI関連 ===

/** トースト表示時間（ミリ秒） */
export const TOAST_DURATION_MS = 2400;

/** ヒント深度のデフォルト値 */
export const HINT_DEPTH_DEFAULT = 8;
