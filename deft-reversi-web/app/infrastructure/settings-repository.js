/**
 * @fileoverview SettingsRepository - ゲーム設定の永続化を担当するリポジトリ
 */

const STORAGE_KEY = 'deft-reversi-settings';
const LEGACY_STORAGE_KEY = 'gameSettings';

/**
 * ゲーム設定の型
 * @typedef {Object} GameSettings
 * @property {boolean} aiEnabled AI有効/無効
 * @property {number} aiLevel AIレベル（1-24）
 * @property {number} analysisLevel 評価レベル（1-24）
 * @property {number} analysisRunLevel 分析レベル（1-24）
 * @property {number} blackAiLevel AI対AI時の黒AIレベル（1-24）
 * @property {number} whiteAiLevel AI対AI時の白AIレベル（1-24）
 * @property {'black' | 'white' | 'both'} aiTurn AIの手番
 * @property {number | null} humanOpening 定石インデックス（未使用はnull）
 */

/**
 * 設定を正規化する
 * @param {any} raw
 * @param {GameSettings} defaults
 * @returns {GameSettings}
 */
function normalizeSettings(raw, defaults) {
    const normalizeLevel = (value, fallback) =>
        Number.isInteger(value) && value >= 1 && value <= 24 ? value : fallback;

    const aiEnabled = typeof raw?.aiEnabled === 'boolean' ? raw.aiEnabled : defaults.aiEnabled;
    const aiLevel = normalizeLevel(raw?.aiLevel, defaults.aiLevel);
    const analysisLevel = normalizeLevel(raw?.analysisLevel, defaults.analysisLevel);
    const analysisRunLevel = normalizeLevel(raw?.analysisRunLevel, defaults.analysisRunLevel);
    const blackAiLevel = normalizeLevel(raw?.blackAiLevel, defaults.blackAiLevel);
    const whiteAiLevel = normalizeLevel(raw?.whiteAiLevel, defaults.whiteAiLevel);
    const aiTurn =
        raw?.aiTurn === 'black' || raw?.aiTurn === 'white' || raw?.aiTurn === 'both'
            ? raw.aiTurn
            : defaults.aiTurn;

    const humanOpeningRaw = raw?.humanOpening;
    const humanOpening =
        humanOpeningRaw === null ||
        humanOpeningRaw === undefined ||
        humanOpeningRaw === 'none' ||
        humanOpeningRaw === ''
            ? null
            : Number.isInteger(humanOpeningRaw)
                ? humanOpeningRaw
                : typeof humanOpeningRaw === 'string' && Number.isInteger(Number(humanOpeningRaw))
                    ? Number(humanOpeningRaw)
                    : null;
    return {
        aiEnabled,
        aiLevel,
        analysisLevel,
        analysisRunLevel,
        blackAiLevel,
        whiteAiLevel,
        aiTurn,
        humanOpening,
    };
}

/**
 * ゲーム設定の永続化を担当するリポジトリ
 */
export class SettingsRepository {
    /**
     * 設定を読み込む（defaultsで補完し、正規化して返す）
     * @param {GameSettings} defaults
     * @returns {GameSettings}
     */
    load(defaults) {
        try {
            const stored = localStorage.getItem(STORAGE_KEY);
            if (stored) {
                const parsed = JSON.parse(stored);
                return normalizeSettings(parsed, defaults);
            }

            // 旧キーから移行（存在する場合のみ）
            const legacyStored = localStorage.getItem(LEGACY_STORAGE_KEY);
            if (legacyStored) {
                const legacyParsed = JSON.parse(legacyStored);
                const normalized = normalizeSettings(legacyParsed, defaults);
                localStorage.setItem(STORAGE_KEY, JSON.stringify(normalized));
                localStorage.removeItem(LEGACY_STORAGE_KEY);
                return normalized;
            }
        } catch (e) {
            console.warn('Failed to load settings:', e);
        }
        return normalizeSettings({}, defaults);
    }

    /**
     * 設定を保存する
     * @param {GameSettings} settings
     */
    save(settings) {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
        } catch (e) {
            console.warn('Failed to save settings:', e);
        }
    }

    /**
     * 設定をリセット
     */
    reset() {
        try {
            localStorage.removeItem(STORAGE_KEY);
        } catch (e) {
            console.warn('Failed to reset settings:', e);
        }
    }
}
