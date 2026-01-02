/**
 * @fileoverview SettingsService - ゲーム設定を管理するアプリケーションサービス
 */

import { SettingsRepository } from '../infrastructure/settings-repository.js';

/**
 * 永続化されるゲーム設定のデフォルト値（仕様）
 */
const DEFAULT_SETTINGS = Object.freeze({
    aiEnabled: true,
    aiLevel: 5,
    aiTurn: 'white',
    humanOpening: null,
});

/**
 * ゲーム設定を管理するアプリケーションサービス
 * インメモリ状態と永続化層を統合
 */
export class SettingsService {
    constructor() {
        /** @private */
        this._repository = new SettingsRepository();
        const saved = this._repository.load(DEFAULT_SETTINGS);

        // AI設定
        /** @private */
        this._enableAi = saved.aiEnabled;
        /** @private */
        this._aiLevel = saved.aiLevel;
        /** @private */
        this._aiTurn = saved.aiTurn;

        // ヒント設定（セッション内のみ、永続化しない）
        /** @private */
        this._enableHint = false;
        /** @private */
        this._hintLevel = 7;

        // その他
        /** @private */
        this._humanOpening = saved.humanOpening;
    }

    // === AI設定 ===

    /** @returns {boolean} */
    get enableAi() {
        return this._enableAi;
    }

    /** @param {boolean} value */
    set enableAi(value) {
        this._enableAi = value;
    }

    /** @returns {number} */
    get aiLevel() {
        return this._aiLevel;
    }

    /** @param {number} value */
    set aiLevel(value) {
        this._aiLevel = value;
    }

    /** @returns {'black' | 'white'} */
    get aiTurn() {
        return this._aiTurn;
    }

    /** @param {'black' | 'white'} value */
    set aiTurn(value) {
        this._aiTurn = value;
    }

    // === ヒント設定 ===

    /** @returns {boolean} */
    get enableHint() {
        return this._enableHint;
    }

    /** @param {boolean} value */
    set enableHint(value) {
        this._enableHint = value;
    }

    /** @returns {number} */
    get hintLevel() {
        return this._hintLevel;
    }

    /** @param {number} value */
    set hintLevel(value) {
        this._hintLevel = value;
    }

    /**
     * ヒント表示を切り替え
     */
    toggleHint() {
        this._enableHint = !this._enableHint;
    }

    // === その他 ===

    /** @returns {number | null} */
    get humanOpening() {
        return this._humanOpening;
    }

    /** @param {number | null} value */
    set humanOpening(value) {
        this._humanOpening = value;
    }

    /**
     * 現在の設定を永続化
     */
    save() {
        this._repository.save({
            aiEnabled: this._enableAi,
            aiLevel: this._aiLevel,
            aiTurn: this._aiTurn,
            humanOpening: this._humanOpening,
        });
    }
}
