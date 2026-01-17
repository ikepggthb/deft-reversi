/**
 * @fileoverview HistoryManager - Undo/Redo履歴を管理するクラス
 */

/**
 * @template T スナップショットの型
 */
export class HistoryManager {
    constructor() {
        /** @private @type {T[]} */
        this._history = [];
        /** @private @type {T[]} */
        this._future = [];
    }

    /**
     * 履歴をクリア
     */
    clear() {
        this._history = [];
        this._future = [];
    }

    /**
     * 履歴に追加
     * @param {T} snapshot
     */
    push(snapshot) {
        this._history.push(snapshot);
    }

    /**
     * 未来をクリア（新しい手を打った時）
     */
    clearFuture() {
        this._future = [];
    }

    /**
     * Undo可能かどうか
     * @returns {boolean}
     */
    get canUndo() {
        return this._history.length > 0;
    }

    /**
     * Redo可能かどうか
     * @returns {boolean}
     */
    get canRedo() {
        return this._future.length > 0;
    }

    /**
     * Undo操作
     * @param {T} currentSnapshot 現在の状態のスナップショット
     * @returns {T | null} 復元するスナップショット、またはnull（Undo不可の場合）
     */
    undo(currentSnapshot) {
        if (this._history.length === 0) return null;
        this._future.push(currentSnapshot);
        return this._history.pop() ?? null;
    }

    /**
     * Redo操作
     * @param {T} currentSnapshot 現在の状態のスナップショット
     * @returns {T | null} 復元するスナップショット、またはnull（Redo不可の場合）
     */
    redo(currentSnapshot) {
        if (this._future.length === 0) return null;
        this._history.push(currentSnapshot);
        return this._future.pop() ?? null;
    }

    /**
     * 履歴の長さ
     * @returns {number}
     */
    get historyLength() {
        return this._history.length;
    }

    /**
     * 未来の長さ
     * @returns {number}
     */
    get futureLength() {
        return this._future.length;
    }
}
