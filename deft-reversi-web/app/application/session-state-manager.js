/**
 * @fileoverview SessionStateManager - セッション状態の管理を担当するモジュール
 */

/**
 * 履歴スナップショット
 * @typedef {Object} HistorySnapshot
 * @property {import('../domain/game.js').Game} game
 * @property {(number | null)[] | null} hintScores
 */

/**
 * セッション状態
 * @typedef {Object} SessionState
 * @property {import('../domain/game.js').Game | null} game
 * @property {HistorySnapshot[]} history
 * @property {HistorySnapshot[]} future
 * @property {(number | null)[] | null} hintScores
 * @property {boolean} pendingAutoStart
 */

/**
 * セッション状態の管理を担当するクラス
 */
export class SessionStateManager {
    constructor() {
        /** @type {SessionState} */
        this._mainState = this._createState();
        /** @type {SessionState | null} */
        this._studyState = null;
        /** @type {string[]} */
        this._studyBranchStartRecord = [];
        /** @type {boolean} */
        this._studyModeEnabled = false;
    }

    /**
     * 新しいセッション状態を作成
     * @returns {SessionState}
     */
    _createState() {
        return {
            game: null,
            history: [],
            future: [],
            hintScores: null,
            pendingAutoStart: false,
        };
    }

    /**
     * セッション状態をクローン
     * @param {SessionState} state
     * @returns {SessionState}
     */
    cloneState(state) {
        return {
            game: state.game,
            history: [...state.history],
            future: [...state.future],
            hintScores: state.hintScores ? [...state.hintScores] : null,
            pendingAutoStart: Boolean(state.pendingAutoStart),
        };
    }

    /**
     * アクティブな状態を取得
     * @returns {SessionState}
     */
    getActiveState() {
        return this._studyModeEnabled && this._studyState ? this._studyState : this._mainState;
    }

    /**
     * メインの状態を取得
     * @returns {SessionState}
     */
    getMainState() {
        return this._mainState;
    }

    /**
     * 検討用の状態を取得
     * @returns {SessionState | null}
     */
    getStudyState() {
        return this._studyState;
    }

    /**
     * 検討モードが有効かどうか
     * @returns {boolean}
     */
    get isStudyModeEnabled() {
        return this._studyModeEnabled;
    }

    /**
     * 検討モードを有効化
     * @param {string[]} mainRecord 本線の棋譜
     */
    enableStudyMode(mainRecord) {
        if (this._studyModeEnabled) return;
        if (!this._mainState.game) return;

        this._studyBranchStartRecord = [...mainRecord];
        this._studyState = this.cloneState(this._mainState);
        this._studyState.pendingAutoStart = false;
        this._studyModeEnabled = true;
    }

    /**
     * 検討モードを無効化
     */
    disableStudyMode() {
        this._studyModeEnabled = false;
        this._studyState = null;
        this._studyBranchStartRecord = [];
    }

    /**
     * 検討状態を破棄する（内部用）
     */
    clearStudyState() {
        this._studyModeEnabled = false;
        this._studyState = null;
        this._studyBranchStartRecord = [];
    }

    /**
     * 検討開始時点の本線棋譜
     * @returns {ReadonlyArray<string>}
     */
    get studyBranchStartRecord() {
        return this._studyBranchStartRecord;
    }

    // === ゲーム状態アクセサ ===

    /** @returns {import('../domain/game.js').Game | null} */
    get game() {
        return this.getActiveState().game;
    }

    /** @param {import('../domain/game.js').Game | null} value */
    set game(value) {
        this.getActiveState().game = value;
    }

    /** @returns {HistorySnapshot[]} */
    get history() {
        return this.getActiveState().history;
    }

    /** @param {HistorySnapshot[]} value */
    set history(value) {
        this.getActiveState().history = value;
    }

    /** @returns {HistorySnapshot[]} */
    get future() {
        return this.getActiveState().future;
    }

    /** @param {HistorySnapshot[]} value */
    set future(value) {
        this.getActiveState().future = value;
    }

    /** @returns {(number | null)[] | null} */
    get hintScores() {
        return this.getActiveState().hintScores;
    }

    /** @param {(number | null)[] | null} value */
    set hintScores(value) {
        this.getActiveState().hintScores = value;
    }

    /** @returns {boolean} */
    get pendingAutoStart() {
        return this.getActiveState().pendingAutoStart;
    }

    /** @param {boolean} value */
    set pendingAutoStart(value) {
        this.getActiveState().pendingAutoStart = Boolean(value);
    }

    // === スナップショット操作 ===

    /**
     * 現在のアクティブ状態のスナップショットを取得
     * @returns {HistorySnapshot}
     */
    snapshot() {
        return this.snapshotState(this.getActiveState());
    }

    /**
     * 指定セッションのスナップショットを取得
     * @param {SessionState} state
     * @returns {HistorySnapshot}
     */
    snapshotState(state) {
        return {
            game: state.game,
            hintScores: state.hintScores ? [...state.hintScores] : null,
        };
    }

    /**
     * 履歴に追加
     */
    pushHistory() {
        const state = this.getActiveState();
        if (state.game) {
            state.history.push(this.snapshotState(state));
        }
    }

    /**
     * 未来を消去
     */
    clearFuture() {
        this.getActiveState().future = [];
    }

    /**
     * スナップショットから復元
     * @param {HistorySnapshot} snapshot
     */
    restore(snapshot) {
        const state = this.getActiveState();
        state.game = snapshot.game;
        state.hintScores = snapshot.hintScores;
    }

    /**
     * メイン状態を完全にリセット
     */
    resetMainState() {
        this._mainState = this._createState();
    }
}
