/**
 * @fileoverview GraphStateManager - 評価グラフの状態管理を担当するモジュール
 */

/**
 * グラフ状態
 * @typedef {Object} GraphState
 * @property {number} token
 * @property {boolean} loading
 * @property {string | null} error
 * @property {boolean} hasAnalysis
 * @property {'live' | 'analysis'} source
 * @property {number} level
 * @property {number[]} series
 * @property {number[]} baseSeries
 * @property {number | null} hover
 * @property {string[]} record
 * @property {string} currentEvalKey
 * @property {number | null} currentEvalValue
 */

/**
 * 評価グラフの状態管理を担当するクラス
 */
export class GraphStateManager {
    /**
     * @param {number} analysisLevel
     */
    constructor(analysisLevel) {
        this._analysisLevel = analysisLevel;
        /** @type {GraphState} */
        this._mainGraph = this._createState();
        /** @type {GraphState | null} */
        this._studyGraph = null;
        /** @type {number | null} */
        this._studyBranchPly = null;
        /** @type {boolean} */
        this._studyMode = false;
    }

    /**
     * 新しいグラフ状態を作成
     * @returns {GraphState}
     */
    _createState() {
        return {
            token: 0,
            loading: false,
            error: null,
            hasAnalysis: true,
            source: 'live',
            level: this._analysisLevel,
            series: [0],
            baseSeries: [0],
            hover: null,
            record: [],
            currentEvalKey: '',
            currentEvalValue: null,
        };
    }

    /**
     * グラフ状態をクローン
     * @param {GraphState} graph
     * @returns {GraphState}
     */
    cloneState(graph) {
        return {
            token: graph.token,
            loading: graph.loading,
            error: graph.error,
            hasAnalysis: graph.hasAnalysis,
            source: graph.source,
            level: graph.level,
            series: [...graph.series],
            baseSeries: [...graph.baseSeries],
            hover: graph.hover,
            record: [...graph.record],
            currentEvalKey: graph.currentEvalKey,
            currentEvalValue: graph.currentEvalValue,
        };
    }

    /**
     * アクティブなグラフを取得
     * @returns {GraphState}
     */
    get activeGraph() {
        return this._studyMode && this._studyGraph ? this._studyGraph : this._mainGraph;
    }

    /**
     * アクティブなグラフを設定
     * @param {GraphState} value
     */
    set activeGraph(value) {
        if (this._studyMode && this._studyGraph) {
            this._studyGraph = value;
            return;
        }
        this._mainGraph = value;
    }

    /**
     * メイングラフを取得
     * @returns {GraphState}
     */
    get mainGraph() {
        return this._mainGraph;
    }

    /**
     * 検討用グラフを取得
     * @returns {GraphState | null}
     */
    get studyGraph() {
        return this._studyGraph;
    }

    /**
     * 検討モードかどうか
     * @returns {boolean}
     */
    get isStudyMode() {
        return this._studyMode;
    }

    /**
     * 検討分岐開始位置
     * @returns {number | null}
     */
    get studyBranchPly() {
        return this._studyBranchPly;
    }

    /**
     * 検討モードを有効化
     * @param {number} branchStartPly
     */
    enterStudyMode(branchStartPly) {
        if (this._studyMode) return;
        this._studyGraph = this.cloneState(this._mainGraph);
        this._studyBranchPly = branchStartPly;
        this._studyMode = true;
    }

    /**
     * 検討モードを無効化
     */
    exitStudyMode() {
        this._studyGraph = null;
        this._studyBranchPly = null;
        this._studyMode = false;
    }

    /**
     * グラフ状態をリセット
     */
    reset() {
        this._studyMode = false;
        this._mainGraph = this._createState();
        this._studyGraph = null;
        this._studyBranchPly = null;
    }

    /**
     * 評価レベルを更新
     * @param {number} level
     */
    setAnalysisLevel(level) {
        this._analysisLevel = level;
        if (this._mainGraph.source === 'live') {
            this._mainGraph.level = level;
        }
    }

    /**
     * カーソル位置を計算
     * @param {GraphState} graph
     * @param {string[]} currentRecord
     * @returns {number | null}
     */
    getCursor(graph, currentRecord) {
        if (graph.source === 'live') {
            if (this._isPrefix(currentRecord, graph.record) && currentRecord.length <= graph.record.length) {
                return Math.min(currentRecord.length, Math.max(0, graph.series.length - 1));
            }
            return null;
        }
        if (this._isPrefix(currentRecord, graph.record) && currentRecord.length <= graph.record.length) {
            return currentRecord.length;
        }
        if (this._isPrefix(graph.record, currentRecord) && this._hasCurrentEvalForRecord(graph, currentRecord)) {
            return graph.series.length - 1;
        }
        return null;
    }

    /**
     * 現在局面がグラフ系列内かどうか
     * @param {GraphState} graph
     * @param {string[]} currentRecord
     * @returns {boolean}
     */
    containsCurrentRecord(graph, currentRecord) {
        if (graph.record.length === 0) return true;
        if (this.getCursor(graph, currentRecord) !== null) return true;
        if (this._isPrefix(currentRecord, graph.record)) return true;
        if (this._isPrefix(graph.record, currentRecord) && this._hasCurrentEvalForRecord(graph, currentRecord)) {
            return true;
        }
        return false;
    }

    /**
     * 棋譜がプレフィックスかどうか
     * @param {string[]} prefixRecord
     * @param {string[]} record
     * @returns {boolean}
     */
    _isPrefix(prefixRecord, record) {
        if (prefixRecord.length > record.length) return false;
        for (let i = 0; i < prefixRecord.length; i++) {
            if (prefixRecord[i] !== record[i]) return false;
        }
        return true;
    }

    /**
     * 公開版: 棋譜がプレフィックスかどうか
     * @param {string[]} prefixRecord
     * @param {string[]} record
     * @returns {boolean}
     */
    isPrefix(prefixRecord, record) {
        return this._isPrefix(prefixRecord, record);
    }

    /**
     * 現在の評価が存在するかチェック
     * @param {GraphState} graph
     * @param {string[]} currentRecord
     * @returns {boolean}
     */
    _hasCurrentEvalForRecord(graph, currentRecord) {
        const key = `${currentRecord.join(' ')}|${graph.level}`;
        return graph.currentEvalKey === key && typeof graph.currentEvalValue === 'number';
    }

    /**
     * パスを含むシリーズを正規化
     * @param {string[]} record
     * @param {number[]} series
     * @returns {number[]}
     */
    normalizePassSeries(record, series) {
        if (!Array.isArray(series) || series.length === 0) return [0];
        const normalized = [...series];
        for (let i = 0; i < record.length; i++) {
            if (String(record[i]).toLowerCase() !== 'pass') continue;
            const targetIndex = i + 1;
            if (targetIndex < normalized.length) {
                normalized[targetIndex] = normalized[Math.max(0, targetIndex - 1)];
            }
        }
        return normalized;
    }

    /**
     * グラフの同期（分析モード用）
     * @param {GraphState} graph
     * @param {string[]} currentRecord
     * @param {(level: number) => Promise<{ ok: true, evalBlack: number } | { ok: false, error: string }>} evaluatePosition
     * @param {number} liveLevel
     * @param {boolean} engineReady
     * @param {() => void} onRender
     */
    async syncGraphState(graph, currentRecord, evaluatePosition, liveLevel, engineReady, onRender) {
        if (!graph.hasAnalysis) {
            if (graph.series.length !== 1 || graph.series[0] !== 0) {
                graph.series = [0];
            }
            graph.loading = false;
            return;
        }

        if (graph.source === 'live') {
            await this._syncLiveGraph(graph, currentRecord, evaluatePosition, liveLevel, engineReady, onRender);
            return;
        }

        if (this._isPrefix(currentRecord, graph.record) && currentRecord.length <= graph.record.length) {
            if (graph.series !== graph.baseSeries) {
                graph.series = graph.baseSeries;
            }
            graph.currentEvalKey = '';
            graph.currentEvalValue = null;
            graph.loading = false;
            return;
        }

        if (!this._isPrefix(graph.record, currentRecord)) {
            return;
        }

        const key = `${currentRecord.join(' ')}|${graph.level}`;
        if (graph.currentEvalKey === key && typeof graph.currentEvalValue === 'number') {
            const extensionLength = Math.max(1, currentRecord.length - graph.record.length);
            graph.series = [
                ...graph.baseSeries,
                ...Array.from({ length: extensionLength }, () => graph.currentEvalValue),
            ];
            graph.loading = false;
            return;
        }
        if (graph.currentEvalKey === key) return;
        if (!engineReady) return;

        graph.currentEvalKey = key;
        graph.loading = true;
        graph.error = null;

        try {
            const res = await evaluatePosition(graph.level);
            if (graph.currentEvalKey !== key) return;
            if (!res.ok) {
                graph.loading = false;
                graph.error = res.error || '評価失敗';
                onRender();
                return;
            }

            graph.currentEvalValue = res.evalBlack;
            const extensionLength = Math.max(1, currentRecord.length - graph.record.length);
            graph.series = [
                ...graph.baseSeries,
                ...Array.from({ length: extensionLength }, () => res.evalBlack),
            ];
            graph.loading = false;
            graph.error = null;
            onRender();
        } catch (error) {
            if (graph.currentEvalKey !== key) return;
            graph.loading = false;
            graph.error = error?.message ?? String(error);
            onRender();
        }
    }

    /**
     * ライブグラフの同期
     * @private
     */
    async _syncLiveGraph(graph, currentRecord, evaluatePosition, liveLevel, engineReady, onRender) {
        if (this._isPrefix(currentRecord, graph.record) && currentRecord.length < graph.record.length) {
            // Keep the plotted future series when navigating backward.
            graph.series = graph.baseSeries;
            graph.currentEvalKey = '';
            graph.currentEvalValue = null;
            graph.loading = false;
            return;
        }

        if (!this._isPrefix(graph.record, currentRecord)) {
            graph.baseSeries = [0];
            graph.series = [0];
            graph.record = [];
            graph.currentEvalKey = '';
            graph.currentEvalValue = null;
        }

        if (currentRecord.length === graph.record.length) {
            if (graph.series !== graph.baseSeries) {
                graph.series = graph.baseSeries;
            }
            graph.loading = false;
            return;
        }

        graph.level = liveLevel;
        const key = `${currentRecord.join(' ')}|${liveLevel}|live`;
        if (graph.currentEvalKey === key) return;
        if (!engineReady) return;

        graph.currentEvalKey = key;
        graph.loading = true;
        graph.error = null;

        try {
            const res = await evaluatePosition(liveLevel);
            if (graph.currentEvalKey !== key) return;
            if (!res.ok) {
                graph.loading = false;
                graph.error = res.error || '評価失敗';
                onRender();
                return;
            }

            graph.currentEvalValue = res.evalBlack;
            const extensionLength = Math.max(1, currentRecord.length - graph.record.length);
            const nextBaseSeries = [
                ...graph.baseSeries,
                ...Array.from({ length: extensionLength }, () => res.evalBlack),
            ];
            graph.baseSeries = this.normalizePassSeries(currentRecord, nextBaseSeries);
            graph.series = graph.baseSeries;
            graph.record = [...currentRecord];
            graph.loading = false;
            graph.error = null;
            onRender();
        } catch (error) {
            if (graph.currentEvalKey !== key) return;
            graph.loading = false;
            graph.error = error?.message ?? String(error);
            onRender();
        }
    }

    /**
     * 検討分岐位置を解決
     * @param {string[]} mainRecord
     * @param {string[]} studyRecord
     * @returns {number}
     */
    resolveStudyBranchPly(mainRecord, studyRecord) {
        const limit = Math.min(mainRecord.length, studyRecord.length);
        let ply = 0;
        while (ply < limit && mainRecord[ply] === studyRecord[ply]) {
            ply += 1;
        }
        return ply;
    }
}
