import { Board } from '../domain/board.js';
import { OPENINGS } from './openings.js';
import { clampInt, escapeHtml, formatRecordText, formatSigned, formatWinnerMessage, renderSparklineSvg } from './utils.js';
import { GraphStateManager } from './graph-state-manager.js';

export class UI {
    /**
     * @param {import('../application/game-service.js').GameService} gameService
     * @param {{ aiEnabled: boolean, aiLevel: number, analysisLevel: number, analysisRunLevel: number, blackAiLevel: number, whiteAiLevel: number, aiTurn: 'black' | 'white' | 'both', humanOpening: number | null }} [initialSettings]
     */
    constructor(gameService, initialSettings) {
        if (!initialSettings) throw new Error('UI requires initialSettings');

        this.gameService = gameService;
        this._initialSettings = initialSettings;

        /** @type {'mode' | 'record' | 'analysis' | 'settings' | null} */
        this._panelOpen = null;
        /** @type {import('../application/game-service.js').GameViewModel | undefined} */
        this._viewModel = undefined;

        this._blackPlayerName = '';
        this._whitePlayerName = '';
        /** @type {'off' | 'ai_white' | 'ai_black' | 'ai_both'} */
        this._playMode = this._initialMode(initialSettings.aiEnabled, initialSettings.aiTurn);
        this._aiLevel = clampInt(initialSettings.aiLevel ?? 8, 1, 24);
        this._analysisLevel = clampInt(initialSettings.analysisLevel ?? 7, 1, 24);
        this._analysisRunLevel = clampInt(initialSettings.analysisRunLevel ?? this._analysisLevel, 1, 24);
        this._blackAiLevel = clampInt(initialSettings.blackAiLevel ?? this._aiLevel, 1, 24);
        this._whiteAiLevel = clampInt(initialSettings.whiteAiLevel ?? this._aiLevel, 1, 24);
        this._humanOpening = initialSettings.humanOpening ?? null;
        this._studyMode = Boolean(this.gameService.isStudyModeEnabled);
        this._showBoardEval = Boolean(this.gameService.hintEnabled);
        this._engineReady = false;
        this._toastTimer = null;
        this._passTimer = null;
        this._resultTimer = null;

        this._graphManager = new GraphStateManager(this._analysisLevel);
        this._analysis = {
            loading: false,
            error: null,
            sourceText: '',
            runLevel: this._analysisRunLevel,
            status: 'idle',
            progressCurrent: 0,
            progressTotal: 0,
            progressRatio: 0,
        };

        this._mount();
        this._render();
        queueMicrotask(() => this._startOrResumeSession());
    }

    get _graph() {
        return this._graphManager.activeGraph;
    }

    set _graph(value) {
        this._graphManager.activeGraph = value;
    }

    get _mainGraph() {
        return this._graphManager.mainGraph;
    }

    get _studyGraph() {
        return this._graphManager.studyGraph;
    }

    get _studyBranchPly() {
        return this._graphManager.studyBranchPly;
    }

    onAIReady() {
        this._engineReady = true;
        this._render();
    }

    logError(message) {
        this._toast(String(message), true);
    }

    logInfo(message) {
        this._toast(String(message), false);
    }

    drawPassMessage() {
        this._showPassModal();
    }

    showEndGameModal(blackCount, whiteCount, blackName, whiteName) {
        const winner =
            blackCount === whiteCount
                ? null
                : blackCount > whiteCount
                  ? 'black'
                  : 'white';
        this._showResultModal(formatWinnerMessage({ winner }, blackName, whiteName));
    }

    render(viewModel, blackPlayerName, whitePlayerName) {
        this._viewModel = viewModel;
        this._blackPlayerName = blackPlayerName ?? '';
        this._whitePlayerName = whitePlayerName ?? '';
        this._render();
    }

    _initialMode(aiEnabled, aiTurn) {
        if (!aiEnabled) return 'off';
        if (aiTurn === 'black') return 'ai_black';
        if (aiTurn === 'both') return 'ai_both';
        return 'ai_white';
    }

    _startOrResumeSession() {
        this._startSessionWithRecord([...(this.gameService.record ?? [])]);
    }

    _startSessionWithRecord(record, overrides = {}) {
        this.gameService.startSession({ ...this._sessionOptions(record), ...overrides });
    }

    _sessionOptions(record) {
        const { aiEnabled, aiTurn, blackPlayerName, whitePlayerName } = this._currentModeConfig();
        return {
            aiEnabled,
            aiLevel: this._aiLevel,
            analysisLevel: this._analysisLevel,
            blackAiLevel: this._blackAiLevel,
            whiteAiLevel: this._whiteAiLevel,
            aiTurn,
            humanOpening: this._humanOpening,
            blackPlayerName,
            whitePlayerName,
            record,
            enableHint: this._showBoardEval,
            hintLevel: this._analysisLevel,
            resetEngine: false,
            pendingAutoStart: this._requiresExplicitStart(record),
        };
    }

    _currentModeConfig() {
        let aiEnabled = false;
        /** @type {'black' | 'white' | 'both'} */
        let aiTurn = 'white';
        let blackPlayerName = 'Black';
        let whitePlayerName = 'White';

        if (this._playMode === 'ai_white') {
            aiEnabled = true;
            aiTurn = 'white';
            blackPlayerName = 'あなた';
            whitePlayerName = `AI Lv ${this._aiLevel}`;
        } else if (this._playMode === 'ai_black') {
            aiEnabled = true;
            aiTurn = 'black';
            blackPlayerName = `AI Lv ${this._aiLevel}`;
            whitePlayerName = 'あなた';
        } else if (this._playMode === 'ai_both') {
            aiEnabled = true;
            aiTurn = 'both';
            blackPlayerName = `AI Lv ${this._blackAiLevel}`;
            whitePlayerName = `AI Lv ${this._whiteAiLevel}`;
        }

        return { aiEnabled, aiTurn, blackPlayerName, whitePlayerName };
    }

    _requiresExplicitStart(record = []) {
        const nextTurn = this._nextTurnForRecord(record);
        if (this._playMode === 'ai_both') return true;
        if (this._playMode === 'ai_black') return nextTurn === 'black';
        if (this._playMode === 'ai_white') return nextTurn === 'white';
        return false;
    }

    _nextTurnForRecord(record = []) {
        const currentRecord = this.gameService.record ?? [];
        if (
            this._viewModel &&
            Array.isArray(record) &&
            record.length === currentRecord.length &&
            record.every((move, index) => move === currentRecord[index])
        ) {
            return this._viewModel.nextTurn ?? 'black';
        }
        return record.length % 2 === 0 ? 'black' : 'white';
    }

    _mount() {
        const root = document.getElementById('app');
        if (!root) throw new Error('#app not found');

        root.innerHTML = `
          <div class="app-shell">
            <header class="topbar">
              <h1>Deft Reversi</h1>
            </header>
            <main class="content" id="content"></main>
            <nav class="dock">
              <button class="dock-btn" data-action="open-panel" data-panel="mode"><span class="dock-icon">[]</span><span class="dock-label">対局</span></button>
              <button class="dock-btn" data-action="open-panel" data-panel="record"><span class="dock-icon">|||</span><span class="dock-label">棋譜</span></button>
              <button class="dock-btn" data-action="open-panel" data-panel="analysis"><span class="dock-icon">##</span><span class="dock-label">分析</span></button>
              <button class="dock-btn" data-action="open-panel" data-panel="settings"><span class="dock-icon">==</span><span class="dock-label">設定</span></button>
            </nav>
          </div>
          <div class="panel-overlay" id="panel-overlay" role="dialog" aria-modal="true" aria-label="panel">
            <div class="panel" id="panel-body"></div>
          </div>
          <div class="pass-modal" id="pass-modal" aria-hidden="true">パス</div>
          <div class="result-modal" id="result-modal" aria-hidden="true"></div>
          <div class="toast" id="toast"></div>
        `;

        this._root = root;
        this._content = /** @type {HTMLDivElement} */ (root.querySelector('#content'));
        this._panelOverlay = /** @type {HTMLDivElement} */ (root.querySelector('#panel-overlay'));
        this._panelBody = /** @type {HTMLDivElement} */ (root.querySelector('#panel-body'));
        this._passModalEl = /** @type {HTMLDivElement} */ (root.querySelector('#pass-modal'));
        this._resultModalEl = /** @type {HTMLDivElement} */ (root.querySelector('#result-modal'));
        this._toastEl = /** @type {HTMLDivElement} */ (root.querySelector('#toast'));

        root.addEventListener('click', (e) => {
            const target = e.target instanceof HTMLElement ? e.target : null;
            if (!target) return;

            const open = target.closest('[data-action="open-panel"]');
            if (open) {
                const panel = open.getAttribute('data-panel');
                if (panel === 'mode' || panel === 'record' || panel === 'analysis' || panel === 'settings') {
                    this._panelOpen = panel;
                    this._renderPanel();
                }
                return;
            }

            if (target === this._panelOverlay || target.closest('[data-click="close-panel"]')) {
                this._panelOpen = null;
                this._renderPanel();
                return;
            }

            const actionEl = target.closest('[data-click]');
            if (!actionEl) return;
            const click = actionEl.getAttribute('data-click');
            if (!click) return;
            this._dispatchClick(click, actionEl);
        });
    }

    _render() {
        this._content.innerHTML = this._renderMain();
        this._wireGraph();
        this._renderPanel();
        this._syncGraphWithCurrentPosition();
    }

    _renderMain() {
        const vm = this._viewModel;
        const blackCount = Board.countBits(vm?.blackBits ?? 0n);
        const whiteCount = Board.countBits(vm?.whiteBits ?? 0n);
        const nextTurn = vm?.nextTurn ?? 'black';
        const blackActive = nextTurn === 'black';
        const whiteActive = nextTurn === 'white';
        const pendingStart = Boolean(vm?.pendingAutoStart) && !this._studyMode;
        const canUndo = this.gameService.canUndo;
        const canRedo = this.gameService.canRedo;
        const startButtonHtml = pendingStart
            ? '<div class="inline-controls__action"><button class="chip chip--primary chip--cta" data-click="start-match">対局開始</button></div>'
            : '';

        return `
          <div class="page">
            <section class="status-line">
              <div class="status-player status-player--black ${blackActive ? 'is-active' : ''}">
                <span class="stone-dot stone-dot--black" aria-hidden="true"></span>
                <div class="status-stack">
                  <span class="status-name">${escapeHtml(this._blackPlayerName || 'BLACK')}</span>
                  <strong class="status-count">${blackCount}</strong>
                </div>
              </div>
              <div class="status-divider" aria-hidden="true"></div>
              <div class="status-player status-player--white ${whiteActive ? 'is-active' : ''}">
                <div class="status-stack status-stack--right">
                  <span class="status-name">${escapeHtml(this._whitePlayerName || 'WHITE')}</span>
                  <strong class="status-count">${whiteCount}</strong>
                </div>
                <span class="stone-dot stone-dot--white" aria-hidden="true"></span>
              </div>
            </section>

            <section class="main-grid">
              <section class="board-area">
                <div class="board-tools">
                  <button class="chip chip--study ${this._studyMode ? 'active' : ''}" data-click="toggle-study-mode">${this._studyMode ? '検討モード ON' : '検討モード OFF'}</button>
                  <button class="chip chip--board-eval ${this._showBoardEval ? 'active' : ''}" data-click="toggle-board-eval">${this._showBoardEval ? '評価値表示 ON' : '評価値表示 OFF'}</button>
                </div>
                ${this._renderBoard(vm)}
                <section class="inline-controls">
                  <div class="inline-controls__nav">
                    <button class="chip chip--nav" data-click="jump-start" aria-label="最初へ" title="最初へ" ${canUndo ? '' : 'disabled'}>≪</button>
                    <button class="chip chip--nav" data-click="undo" aria-label="戻る" title="戻る" ${canUndo ? '' : 'disabled'}>‹</button>
                    <button class="chip chip--nav" data-click="redo" aria-label="進む" title="進む" ${canRedo ? '' : 'disabled'}>›</button>
                    <button class="chip chip--nav" data-click="jump-end" aria-label="最後へ" title="最後へ" ${canRedo ? '' : 'disabled'}>≫</button>
                  </div>
                  ${startButtonHtml}
                </section>
              </section>
              ${this._renderGraph()}
            </section>
          </div>
        `;
    }

    _renderGraph() {
        const activeGraph = this._graph;
        const baseGraph = this._studyMode && this._studyGraph ? this._mainGraph : activeGraph;
        const studyBranchPly = this._studyMode && this._studyGraph ? this._resolveStudyBranchPly() : null;
        const currentRecord = this._studyMode && this._studyGraph
            ? [...(this.gameService.record ?? [])]
            : [...(this.gameService.mainRecord ?? this.gameService.record ?? [])];
        const series = activeGraph.series;
        const cursor = this._graphCursor(activeGraph, currentRecord);
        const hover = activeGraph.hover;
        const resolvedCursor = cursor === null ? Math.max(0, series.length - 1) : cursor;
        const cursorEval =
            series.length > 0 && resolvedCursor >= 0 && resolvedCursor < series.length
                ? series[resolvedCursor]
                : series.at(-1) ?? 0;
        const hoverEval =
            hover !== null && hover >= 0 && hover < series.length ? series[hover] : null;
        const overlaySeries =
            this._studyMode && this._studyGraph
                ? activeGraph.series.slice(Math.max(0, studyBranchPly ?? 0))
                : null;
        const svg = renderSparklineSvg(baseGraph.series, {
            cursor: this._studyMode && this._studyGraph ? undefined : (cursor ?? undefined),
            hover: this._studyMode && this._studyGraph ? null : hover,
            overlaySeries,
            overlayStart: this._studyMode && this._studyGraph ? Math.max(0, studyBranchPly ?? 0) : undefined,
            overlayCursor: this._studyMode && this._studyGraph ? (cursor ?? undefined) : undefined,
            overlayHover: this._studyMode && this._studyGraph ? hover : null,
            marker: this._studyMode && this._studyGraph ? Math.max(0, studyBranchPly ?? 0) : undefined,
        });
        const statusText = activeGraph.loading
            ? '解析中...'
            : activeGraph.error
              ? escapeHtml(activeGraph.error)
              : `評価: ${formatSigned(cursorEval)}`;
        const relationText = this._graphRelationText(activeGraph, currentRecord, cursor);
        const graphBadge = this._renderGraphStatusBadge();
        const graphProgress = this._renderAnalysisProgress('graph');

        return `
          <section class="graph-area">
            <div class="graph-head">
              <div class="graph-head__title">
                <strong>評価グラフ</strong>
                ${graphBadge}
                ${this._studyMode && this._studyGraph ? '<span class="status-badge status-badge--study">検討プロット</span>' : ''}
              </div>
              <span class="mini">評価 Lv ${activeGraph.level}</span>
            </div>
            ${graphProgress}
            ${this._studyMode && this._studyGraph ? '<div class="graph-legend"><span class="graph-legend__item graph-legend__item--main">本線</span><span class="graph-legend__item graph-legend__item--study">検討</span></div>' : ''}
            <div class="graph" id="eval-graph">${svg}</div>
            <div class="mini" style="margin-top:8px;">
              ${statusText}${hoverEval !== null ? ` / 位置: ${formatSigned(hoverEval)}` : ''}${relationText}
            </div>
          </section>
        `;
    }

    _renderGraphStatusBadge() {
        if (this._analysis.loading) {
            return `<span class="status-badge status-badge--running">解析中 ${escapeHtml(this._analysisProgressLabel())}</span>`;
        }
        return '';
    }

    _analysisProgressLabel() {
        const total = Math.max(0, Math.trunc(Number(this._analysis.progressTotal) || 0));
        const current = Math.max(0, Math.trunc(Number(this._analysis.progressCurrent) || 0));
        return `${current}/${total}`;
    }

    _renderAnalysisProgress(context = 'panel') {
        if (!this._analysis.loading) return '';
        const ratio = Math.max(0, Math.min(1, Number(this._analysis.progressRatio) || 0));
        const label = escapeHtml(this._analysisProgressLabel());
        const width = `${Math.round(ratio * 100)}%`;
        const extraClass = context === 'graph' ? ' progress-meter--graph' : '';
        return `
          <div class="progress-meter${extraClass}" aria-label="analysis-progress">
            <div class="progress-meter__label">進捗 ${label}</div>
            <div class="progress-meter__track"><div class="progress-meter__fill" style="width:${width};"></div></div>
          </div>
        `;
    }

    _setStudyMode(enabled) {
        const next = Boolean(enabled);
        if (next === this._studyMode) return;
        if (next) {
            const branchStartPly = (this.gameService.record ?? []).length;
            this._graphManager.enterStudyMode(branchStartPly);
        } else {
            this._graphManager.exitStudyMode();
        }
        this._studyMode = next;
        this.gameService.setStudyMode(next);
    }

    _resolveStudyBranchPly() {
        const mainRecord = [...(this.gameService.mainRecord ?? [])];
        const studyRecord = [...(this.gameService.record ?? [])];
        return this._graphManager.resolveStudyBranchPly(mainRecord, studyRecord);
    }

    _graphCursor(graph, currentRecord) {
        return this._graphManager.getCursor(graph, currentRecord);
    }

    _graphRelationText(graph, currentRecord, cursor) {
        if (cursor !== null) return '';
        if (graph.record.length === 0) return '';
        if (graph.source === 'live') {
            return '';
        }
        if (this._graphManager.isPrefix(graph.record, currentRecord)) {
            return ' / 現在局面を末尾に追加表示中';
        }
        if (this._graphManager.containsCurrentRecord(graph, currentRecord)) {
            return '';
        }
        return ' / 現在局面は解析系列の外です';
    }

    _renderBoard(vm) {
        const legalBits = vm?.legalMovesBits ?? 0n;
        const blackBits = vm?.blackBits ?? 0n;
        const whiteBits = vm?.whiteBits ?? 0n;
        const lastMove = vm?.lastMove ?? null;
        const nextOpeningPos = vm?.humanOpeningNextPosition ?? null;
        const evalScores = vm?.eval ?? null;
        const bestEval = this._bestLegalEval(evalScores, legalBits);

        const letters = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];
        const numbers = ['1', '2', '3', '4', '5', '6', '7', '8'];
        const tiles = [];

        for (let row = 0; row < 10; row++) {
            for (let col = 0; col < 10; col++) {
                if ((row === 0 || row === 9) && (col === 0 || col === 9)) {
                    tiles.push('<div class="coord"></div>');
                    continue;
                }
                if ((row === 0 || row === 9) && col >= 1 && col <= 8) {
                    tiles.push(`<div class="coord">${letters[col - 1]}</div>`);
                    continue;
                }
                if ((col === 0 || col === 9) && row >= 1 && row <= 8) {
                    tiles.push(`<div class="coord">${numbers[row - 1]}</div>`);
                    continue;
                }

                const x = col - 1;
                const y = row - 1;
                const pos = y * 8 + x;
                const isLegal = ((legalBits >> BigInt(pos)) & 1n) === 1n;
                const isBlack = ((blackBits >> BigInt(pos)) & 1n) === 1n;
                const isWhite = ((whiteBits >> BigInt(pos)) & 1n) === 1n;
                const isLast = lastMove === pos;
                const isNextOpening = nextOpeningPos === pos;
                const evalValue = Array.isArray(evalScores) ? evalScores[pos] : null;
                const isBest = isLegal && typeof evalValue === 'number' && bestEval !== null && evalValue === bestEval;

                const classes = [
                    'cell',
                    isLegal ? 'legal' : '',
                    isLast ? 'last-move' : '',
                    isNextOpening ? 'next-opening' : '',
                ]
                    .filter(Boolean)
                    .join(' ');

                const stone = isBlack
                    ? '<div class="stone black"></div>'
                    : isWhite
                      ? '<div class="stone white"></div>'
                      : '';

                const legalOverlay =
                    this._showBoardEval && isLegal && typeof evalValue === 'number'
                        ? `<span class="cell-eval ${isBest ? 'cell-eval--best' : ''}">${escapeHtml(formatSigned(evalValue))}</span>`
                        : '';

                tiles.push(`
                  <button class="${classes}" data-click="cell" data-pos="${pos}" aria-label="cell ${pos}">
                    ${stone}
                    ${legalOverlay}
                  </button>
                `);
            }
        }

        return `<div class="board-shell" role="grid">${tiles.join('')}</div>`;
    }

    _bestLegalEval(evalScores, legalBits) {
        if (!Array.isArray(evalScores)) return null;
        let best = null;
        for (let pos = 0; pos < 64; pos++) {
            if (((legalBits >> BigInt(pos)) & 1n) !== 1n) continue;
            const value = evalScores[pos];
            if (typeof value !== 'number') continue;
            if (best === null || value > best) best = value;
        }
        return best;
    }

    _renderPanel() {
        if (!this._panelOverlay || !this._panelBody) return;
        const open = this._panelOpen !== null;
        this._panelOverlay.classList.toggle('open', open);
        if (!open) return;

        switch (this._panelOpen) {
            case 'mode':
                this._panelBody.innerHTML = this._renderModePanel();
                this._wireModePanel();
                break;
            case 'record':
                this._panelBody.innerHTML = this._renderRecordPanel();
                break;
            case 'analysis':
                this._panelBody.innerHTML = this._renderAnalysisPanel();
                this._wireAnalysisPanel();
                break;
            case 'settings':
                this._panelBody.innerHTML = this._renderSettingsPanel();
                this._wireSettingsPanel();
                break;
            default:
                break;
        }
    }

    _renderModePanel() {
        const mode = this._playMode;
        const aiLevelOptions = this._renderLevelOptions(this._aiLevel);
        const blackAiLevelOptions = this._renderLevelOptions(this._blackAiLevel);
        const whiteAiLevelOptions = this._renderLevelOptions(this._whiteAiLevel);
        const openingOptions = [
            '<option value="none">No Opening</option>',
            ...OPENINGS.map(([idx, name]) => `<option value="${idx}">${escapeHtml(String(name))}</option>`),
        ].join('');
        const chip = (value, label) =>
            `<button class="mode-chip ${mode === value ? 'active' : ''}" data-click="mode" data-mode="${value}">${label}</button>`;
        const duelLevels =
            this._playMode === 'ai_both'
                ? `
                  <div class="panel-section mode-panel-section">
                    <div class="panel-section-title">AIレベル設定</div>
                    ${this._renderSelectRow('黒番 (Black)', 'mode-black-ai-level-select', blackAiLevelOptions, 'stone-label stone-label--black')}
                    ${this._renderSelectRow('白番 (White)', 'mode-white-ai-level-select', whiteAiLevelOptions, 'stone-label stone-label--white')}
                  </div>
                `
                : this._playMode === 'off'
                  ? ''
                  : `
                  <div class="panel-section mode-panel-section">
                    <div class="panel-section-title">AIレベル設定</div>
                    ${this._renderSelectRow(
                        this._playMode === 'ai_black' ? '黒番 (Black)' : '白番 (White)',
                        'mode-ai-level-select',
                        aiLevelOptions,
                        this._playMode === 'ai_black' ? 'stone-label stone-label--black' : 'stone-label stone-label--white'
                    )}
                  </div>
                `;
        const openingLabel = this._humanOpening === null
            ? '定石タイプ: なし'
            : `定石タイプ: ${escapeHtml(String(OPENINGS.find(([idx]) => idx === this._humanOpening)?.[1] ?? 'なし'))}`;

        return `
          ${this._renderPanelHead('対局設定')}
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">対局モード</div>
            <div class="mode-chip-grid">
              ${chip('off', 'AI無効')}
              ${chip('ai_white', 'AIは白番')}
              ${chip('ai_black', 'AIは黒番')}
              ${chip('ai_both', 'AI対AI')}
            </div>
          </div>
          ${duelLevels}
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">定石設定</div>
            <div class="select-label">${openingLabel}</div>
            <select id="mode-opening-select" class="select">${openingOptions}</select>
          </div>
          <div class="panel-section mode-panel-section mode-panel-section--flat">
            <div class="mode-panel-actions">
              <button class="secondary-btn secondary-btn--hero" data-click="new-board">新規盤面で開始</button>
              <button class="secondary-btn" data-click="close-panel">閉じる</button>
            </div>
          </div>
        `;
    }

    _renderPanelHead(title) {
        return `
          <div class="panel-head panel-head--mode">
            <strong>${escapeHtml(title)}</strong>
            <button class="icon-btn icon-btn--soft" data-click="close-panel" aria-label="閉じる"><span class="close-glyph">×</span></button>
          </div>
        `;
    }

    _wireModePanel() {
        const openingSelect = this._panelBody.querySelector('#mode-opening-select');
        if (openingSelect instanceof HTMLSelectElement) {
            openingSelect.value = this._humanOpening === null ? 'none' : String(this._humanOpening);
            openingSelect.onchange = () => {
                const v = openingSelect.value;
                this._humanOpening = v === 'none' ? null : Number(v);
                this.gameService.setHumanOpening(this._humanOpening);
            };
        }

        const bindSelect = (selectId, onChange) => {
            const select = this._panelBody.querySelector(`#${selectId}`);
            if (!(select instanceof HTMLSelectElement)) return;
            select.onchange = () => {
                const next = clampInt(Number(select.value), 1, 24);
                onChange(next);
            };
        };

        bindSelect('mode-ai-level-select', (next) => {
            this._aiLevel = next;
            this._syncAiSettings();
        });
        bindSelect('mode-black-ai-level-select', (next) => {
            this._blackAiLevel = next;
            this._syncAiSettings();
        });
        bindSelect('mode-white-ai-level-select', (next) => {
            this._whiteAiLevel = next;
            this._syncAiSettings();
        });
    }

    _renderRecordPanel() {
        const text = formatRecordText(this.gameService.record ?? []);
        return `
          ${this._renderPanelHead('棋譜')}
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">棋譜入力</div>
            <textarea id="record-input" class="textarea" placeholder="例: f5 d6 c3 ... (または f5d6c3)">${escapeHtml(text)}</textarea>
            <div class="row" style="margin-top:10px;">
              <button class="secondary-btn" data-click="load-record">読み込み</button>
              <button class="secondary-btn" data-click="copy-record-input">コピー</button>
            </div>
          </div>
        `;
    }

    _renderAnalysisPanel() {
        const text = this._analysis.sourceText || (this.gameService.record ?? []).join(' ');
        const levelOptions = this._renderLevelOptions(this._analysis.runLevel);
        const actionLabel = this._analysis.loading ? '解析中...' : '解析';
        const status = this._analysis.loading
            ? this._renderAnalysisProgress('panel')
            : this._analysis.error
              ? `<div class="mini" style="margin-top:8px; color:#dc2626;">${escapeHtml(this._analysis.error)}</div>`
              : '';

        return `
          ${this._renderPanelHead('分析')}
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">分析レベル</div>
            <select id="analysis-run-level-select" class="select">${levelOptions}</select>
          </div>
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">分析対象</div>
            <textarea id="analysis-input" class="textarea" placeholder="例: f5 d6 c3 ... (または f5d6c3)">${escapeHtml(text)}</textarea>
            <div class="row" style="margin-top:10px;">
              <button class="secondary-btn ${this._analysis.loading ? 'secondary-btn--busy' : ''}" data-click="run-analysis" ${this._analysis.loading ? 'disabled' : ''}>${actionLabel}</button>
            </div>
            ${status}
          </div>
        `;
    }

    _wireAnalysisPanel() {
        const levelSelect = this._panelBody.querySelector('#analysis-run-level-select');
        if (levelSelect instanceof HTMLSelectElement) {
            levelSelect.value = String(this._analysis.runLevel);
            levelSelect.onchange = () => {
                this._analysis.runLevel = clampInt(Number(levelSelect.value), 1, 24);
                this.gameService.setAnalysisRunLevel(this._analysis.runLevel);
            };
        }

        const input = this._panelBody.querySelector('#analysis-input');
        if (!(input instanceof HTMLTextAreaElement)) return;
        input.oninput = () => {
            this._analysis.sourceText = input.value;
        };
    }

    _renderSettingsPanel() {
        const analysisLevelOptions = this._renderLevelOptions(this._analysisLevel);

        return `
          ${this._renderPanelHead('設定')}
          <div class="panel-section mode-panel-section">
            <div class="panel-section-title">評価レベル</div>
            ${this._renderSelectRow('盤面評価', 'analysis-level-select', analysisLevelOptions)}
          </div>
          <div class="panel-section mode-panel-section">
            <button class="secondary-btn" data-click="rotate-board">90° 回転</button>
          </div>
        `;
    }

    _renderSelectRow(label, selectId, options, labelClass = '') {
        return `
          <div class="setting-row">
            <div class="${labelClass || 'mini'}" style="margin-bottom:8px;">${escapeHtml(label)}</div>
            <select id="${selectId}" class="select">${options}</select>
          </div>
        `;
    }

    _renderLevelOptions(selected) {
        return Array.from({ length: 24 }, (_, i) => {
            const value = i + 1;
            return `<option value="${value}" ${value === selected ? 'selected' : ''}>Lv ${value}</option>`;
        }).join('');
    }

    _wireSettingsPanel() {
        const bindSelect = (selectId, onChange) => {
            const select = this._panelBody.querySelector(`#${selectId}`);
            if (!(select instanceof HTMLSelectElement)) return;
            select.onchange = () => {
                const next = clampInt(Number(select.value), 1, 24);
                onChange(next);
            };
        };

        bindSelect('analysis-level-select', (next) => {
            this._analysisLevel = next;
            this._graphManager.setAnalysisLevel(next);
            this.gameService.setAnalysisLevel(next);
            this._render();
        });
    }

    _syncAiSettings() {
        const { aiEnabled, aiTurn, blackPlayerName, whitePlayerName } = this._currentModeConfig();
        this.gameService.updateAISettings(
            aiEnabled,
            this._aiLevel,
            aiTurn,
            this._blackAiLevel,
            this._whiteAiLevel
        );
        this.gameService.setPlayerNames(blackPlayerName, whitePlayerName);
        this._render();
    }

    _dispatchClick(click, el) {
        const handler = this._getClickHandler(click);
        if (handler) handler(el);
    }

    _getClickHandler(click) {
        const handlers = {
            'cell': (el) => {
                const pos = Number(el.getAttribute('data-pos'));
                if (Number.isFinite(pos)) {
                    this.gameService.handleBoardClick(pos);
                }
            },
            'undo': () => this.gameService.undo(),
            'redo': () => this.gameService.redo(),
            'jump-start': () => this.gameService.undoToStart(),
            'jump-end': () => this.gameService.redoToEnd(),
            'toggle-study-mode': () => {
                this._setStudyMode(!this._studyMode);
                this._render();
            },
            'toggle-board-eval': () => {
                this._showBoardEval = !this._showBoardEval;
                this.gameService.setBoardEvalEnabled(this._showBoardEval);
                this._render();
            },
            'ai-move': () => this.gameService.playAiMoveOnce(this._manualAiLevel()),
            'start-match': () => this.gameService.startAutoPlay(),
            'mode': (el) => this._handleModeClick(el),
            'load-record': () => this._handleLoadRecord(),
            'copy-record-input': () => this._handleCopyRecordInput(),
            'run-analysis': () => this._handleRunAnalysis(),
            'new-board': () => {
                this._setStudyMode(false);
                this._resetGraph();
                this.gameService.startNewGame();
                this._panelOpen = null;
                this._renderPanel();
            },
            'rotate-board': () => {
                this._setStudyMode(false);
                this._resetGraph();
                this.gameService.transformBoard({ rotateQuarterTurns: 1 });
            },
        };
        return handlers[click];
    }

    _handleModeClick(el) {
        const mode = el.getAttribute('data-mode');
        if (mode !== 'off' && mode !== 'ai_white' && mode !== 'ai_black' && mode !== 'ai_both') return;
        this._setStudyMode(false);
        this._playMode = mode;
        this._resetGraph();
        this._startSessionWithRecord([...(this.gameService.record ?? [])], {
            runAiOnStart: false,
            pendingAutoStart: mode !== 'off',
        });
        this._render();
    }

    _handleLoadRecord() {
        this._setStudyMode(false);
        const input = this._panelBody.querySelector('#record-input');
        const text = input instanceof HTMLTextAreaElement ? input.value : formatRecordText(this.gameService.record ?? []);
        const parsed = this.gameService.parseMoveList(text ?? '');
        if (!parsed.ok) {
            this._toast(parsed.error, true);
            return;
        }
        this._resetGraph();
        this._startSessionWithRecord(parsed.record, { runAiOnStart: false });
        this._panelOpen = null;
        this._renderPanel();
    }

    _handleCopyRecordInput() {
        const input = this._panelBody.querySelector('#record-input');
        const text = input instanceof HTMLTextAreaElement ? input.value : formatRecordText(this.gameService.record ?? []);
        this._copyText(text ?? '');
    }

    _handleRunAnalysis() {
        const input = this._panelBody.querySelector('#analysis-input');
        const text = input instanceof HTMLTextAreaElement ? input.value : this._analysis.sourceText;
        this._analysis.sourceText = text ?? '';
        this._runAnalysisFromText(this._analysis.sourceText);
    }

    _manualAiLevel() {
        if (this._playMode !== 'ai_both') return this._aiLevel;
        return this._viewModel?.nextTurn === 'black' ? this._blackAiLevel : this._whiteAiLevel;
    }

    _resetAnalysisProgress() {
        this._analysis.progressCurrent = 0;
        this._analysis.progressTotal = 0;
        this._analysis.progressRatio = 0;
    }

    _runAnalysisFromText(text) {
        const parsed = this.gameService.parseMoveList(text ?? '');
        if (!parsed.ok) {
            this._analysis.loading = false;
            this._analysis.error = parsed.error;
            this._analysis.status = 'error';
            this._resetAnalysisProgress();
            this._renderPanel();
            return;
        }

        this._analysis.loading = true;
        this._analysis.error = null;
        this._analysis.status = 'running';
        this._analysis.progressCurrent = 0;
        this._analysis.progressTotal = parsed.record.length + 1;
        this._analysis.progressRatio = 0;
        this._graph.loading = true;
        this._graph.error = null;
        const token = ++this._graph.token;
        this._render();
        this._renderPanel();

        this.gameService
            .analyzeRecord(parsed.record, this._analysis.runLevel, {
                onProgress: ({ completed, total }) => {
                    if (token !== this._graph.token) return;
                    this._analysis.progressCurrent = Math.max(0, Math.trunc(Number(completed) || 0));
                    this._analysis.progressTotal = Math.max(0, Math.trunc(Number(total) || 0));
                    const denominator = Math.max(1, this._analysis.progressTotal);
                    this._analysis.progressRatio = Math.max(
                        0,
                        Math.min(1, this._analysis.progressCurrent / denominator)
                    );
                    this._render();
                    this._renderPanel();
                },
            })
            .then((res) => {
                if (token !== this._graph.token) return;
                this._analysis.loading = false;
                if (!res.ok) {
                    this._analysis.error = res.error || '解析失敗';
                    this._analysis.status = 'error';
                    this._resetAnalysisProgress();
                    this._graph.loading = false;
                    this._graph.error = this._analysis.error;
                    this._render();
                    this._renderPanel();
                    return;
                }

                this._analysis.error = null;
                this._analysis.status = 'done';
                this._resetAnalysisProgress();
                const nextSeries = this._normalizePassSeries(
                    parsed.record,
                    res.analysis.map((x) => x.evalBlack)
                );
                this._graph.hasAnalysis = true;
                this._graph.source = 'analysis';
                this._graph.level = this._analysis.runLevel;
                this._graph.baseSeries = nextSeries;
                this._graph.series = nextSeries;
                this._graph.record = [...parsed.record];
                this._graph.currentEvalKey = '';
                this._graph.currentEvalValue = null;
                this._graph.loading = false;
                this._graph.error = null;
                this._panelOpen = null;
                this._render();
                this._renderPanel();
            })
            .catch((error) => {
                if (token !== this._graph.token) return;
                this._analysis.loading = false;
                this._analysis.error = error?.message ?? String(error);
                this._analysis.status = 'error';
                this._resetAnalysisProgress();
                this._graph.loading = false;
                this._graph.error = this._analysis.error;
                this._render();
                this._renderPanel();
            });
    }

    _syncGraphWithCurrentPosition() {
        const mainRecord = [...(this.gameService.mainRecord ?? [])];
        this._graphManager.syncGraphState(
            this._mainGraph,
            mainRecord,
            (level) => this.gameService.evaluatePosition(level, { scope: 'main' }),
            this._analysisLevel,
            this._engineReady,
            () => this._render()
        );

        if (this._studyMode && this._studyGraph) {
            const studyRecord = [...(this.gameService.record ?? [])];
            this._graphManager.syncGraphState(
                this._studyGraph,
                studyRecord,
                (level) => this.gameService.evaluatePosition(level, { scope: 'study' }),
                this._analysisLevel,
                this._engineReady,
                () => this._render()
            );
        }
    }

    _normalizePassSeries(record, series) {
        return this._graphManager.normalizePassSeries(record, series);
    }

    _resetGraph() {
        this._studyMode = false;
        this._graphManager.reset();
    }

    _wireGraph() {
        const graph = this._content.querySelector('#eval-graph');
        if (!(graph instanceof HTMLElement)) return;
        const resolvePly = (clientX) => {
            const n = this._graph.series.length - 1;
            if (n < 0) return null;
            const rect = graph.getBoundingClientRect();
            const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
            return Math.round((x / Math.max(1, rect.width)) * n);
        };

        graph.onmousemove = (ev) => {
            const ply = resolvePly(ev.clientX);
            if (ply === null || ply <= 0 && this._graph.series.length <= 1) return;
            if (this._graph.hover !== ply) {
                this._graph.hover = ply;
                this._render();
            }
        };

        graph.onmouseleave = () => {
            if (this._graph.hover !== null) {
                this._graph.hover = null;
                this._render();
            }
        };
        graph.onclick = (ev) => {
            const ply = resolvePly(ev.clientX);
            if (ply === null) return;
            this.gameService.goToPly(ply);
        };
    }

    async _copyText(text) {
        try {
            await navigator.clipboard.writeText(text ?? '');
            this._toast('コピーしました', false);
        } catch (_error) {
            this._toast('コピーに失敗しました', true);
        }
    }

    _toast(message, isError) {
        if (!this._toastEl) return;
        this._toastEl.textContent = message;
        this._toastEl.classList.toggle('error', Boolean(isError));
        this._toastEl.classList.add('show');
        if (this._toastTimer) clearTimeout(this._toastTimer);
        this._toastTimer = setTimeout(() => this._toastEl?.classList.remove('show'), 1400);
    }

    _showPassModal() {
        if (!this._passModalEl) return;
        this._passModalEl.classList.add('show');
        this._passModalEl.setAttribute('aria-hidden', 'false');
        if (this._passTimer) clearTimeout(this._passTimer);
        this._passTimer = setTimeout(() => {
            this._passModalEl?.classList.remove('show');
            this._passModalEl?.setAttribute('aria-hidden', 'true');
        }, 1000);
    }

    _showResultModal(message) {
        if (!this._resultModalEl) return;
        this._resultModalEl.textContent = message;
        this._resultModalEl.classList.add('show');
        this._resultModalEl.setAttribute('aria-hidden', 'false');
        if (this._resultTimer) clearTimeout(this._resultTimer);
        this._resultTimer = setTimeout(() => {
            this._resultModalEl?.classList.remove('show');
            this._resultModalEl?.setAttribute('aria-hidden', 'true');
        }, 1000);
    }
}
