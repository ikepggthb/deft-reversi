import { Board } from '../domain/board.js';
import { Turn } from '../domain/types.js';
import { OPENINGS } from './openings.js';

/**
 * 新UI（DOMベース）
 * - タイトル: Play vs AI / Free Mode / Analysis
 * - 対戦: 設定 → 対局 → 結果 → 分析へ遷移
 * - Free/Analysis: 盤面操作 + 棋譜入力 + 評価表示（ヒント）
 */
export class UI {
    /**
     * @param {import('../application/game-service.js').GameService} gameService
     * @param {{ aiEnabled: boolean, aiLevel: number, aiTurn: 'black' | 'white', humanOpening: number | null }} [initialSettings]
     */
    constructor(gameService, initialSettings) {
        if (!initialSettings) {
            throw new Error('UI requires initialSettings');
        }

        this.gameService = gameService;
        this._initialSettings = initialSettings;

        /** @type {'home' | 'match-setup' | 'match' | 'free' | 'analysis-setup' | 'analysis' | 'result'} */
        this._route = 'home';
        this._engineReady = false;
        this._settingsOpen = false;
        this._moveListOpen = false;

        /** @type {import('../application/game-service.js').GameViewModel | undefined} */
        this._viewModel = undefined;
        this._blackPlayerName = '';
        this._whitePlayerName = '';

        this._result = null;
        this._resultRecordText = '';

        this._setup = {
            tab: 'preset',
            playerColor: 'black',
            preset: 'intermediate',
            aiLevel: clampInt(this._initialSettings.aiLevel ?? 8, 1, 24),
            humanOpening: this._initialSettings.humanOpening ?? null,
            moveList: '',
        };

        this._free = {
            hintDepth: 8,
            aiLevel: 8,
            moveList: '',
        };

        this._analysis = {
            hintDepth: 8,
            aiLevel: 8,
            moveList: '',
            tab: 'graph',
            cursorPly: 0,
            isAnalyzing: false,
            /** @type {{ record: string[], analysis: Array<{ ply: number, nextTurn: import('../domain/types.js').Turn, evalToMove: number, evalBlack: number, bestMove: string | null }> } | null} */
            analysis: null,
            /** @type {number | null} */
            hoverPly: null,
            /** @type {'play' | 'edit'} */
            mode: 'play',
            /** @type {number} */
            scenarioPly: 0,
            /** @type {number} */
            scenarioBasePly: 0,
        };

        this._mount();
        this._render();
    }

    // === GameService callbacks ===

    onAIReady() {
        this._engineReady = true;
        this._updateEnginePill();
    }

    logError(message) {
        this._toast(String(message), true);
    }

    logInfo(message) {
        this._toast(String(message), false);
    }

    drawPassMessage() {
        this._toast('PASS', false);
    }

    /**
     * @param {number} blackScore
     * @param {number} whiteScore
     * @param {string} blackPlayerName
     * @param {string} whitePlayerName
     * @param {string} recordText
     */
    showEndGameModal(blackScore, whiteScore, blackPlayerName, whitePlayerName, recordText) {
        // Result画面は「対戦」専用。分析/Freeでは画面遷移せず通知のみ。
        if (this._route !== 'match') {
            const winner =
                blackScore === whiteScore ? 'DRAW' : blackScore > whiteScore ? 'BLACK' : 'WHITE';
            this._toast(`Game Over (${winner})`, false);
            return;
        }

        this._result = { blackScore, whiteScore, blackPlayerName, whitePlayerName };
        this._resultRecordText = recordText ?? '';
        this._setRoute('result');
    }

    /**
     * @param {import('../application/game-service.js').GameViewModel | undefined} viewModel
     * @param {string} blackPlayerName
     * @param {string} whitePlayerName
     */
    render(viewModel, blackPlayerName, whitePlayerName) {
        this._viewModel = viewModel;
        this._blackPlayerName = blackPlayerName ?? '';
        this._whitePlayerName = whitePlayerName ?? '';

        // 分析画面では、Undo/Redo/編集の結果がカーソルやグラフに反映されるよう同期する
        if (this._route === 'analysis' && this._analysis.analysis?.record) {
            const currentPly = (this.gameService.record ?? []).length;
            this._analysis.scenarioPly = currentPly;
            if (this._analysis.mode === 'play') {
                const maxPly = this._analysis.analysis.record.length;
                this._analysis.cursorPly = Math.max(0, Math.min(currentPly, maxPly));
            }
        }

        this._render();
    }

    // === Mount / Layout ===

    _mount() {
        const root = document.getElementById('app');
        if (!root) throw new Error('#app not found');

        root.innerHTML = `
            <div class="app-shell">
              <div class="topbar">
                <button class="icon-btn" data-action="back" aria-label="back">←</button>
                <h1>Deft Reversi</h1>
                <button class="icon-btn" data-action="settings" aria-label="settings">⚙</button>
              </div>
              <div class="content" id="content"></div>
            </div>
            <div class="toast" id="toast"></div>
            <div class="modal-overlay" id="settings-overlay" role="dialog" aria-modal="true" aria-label="settings">
              <div class="modal">
                <div class="modal-title">
                  <strong>SETTINGS</strong>
                  <button class="icon-btn" data-action="close-settings" aria-label="close">✕</button>
                </div>
                <div id="settings-body"></div>
              </div>
            </div>
            <div class="modal-overlay" id="movelist-overlay" role="dialog" aria-modal="true" aria-label="move list input">
              <div class="modal">
                <div class="modal-title">
                  <strong>MOVE LIST</strong>
                  <button class="icon-btn" data-action="close-movelist" aria-label="close">✕</button>
                </div>
                <textarea class="textarea" id="movelist-text" placeholder="e.g. f5 d6 c3 ... (or f5d6c3)"></textarea>
                <div class="row" style="margin-top:10px;">
                  <button class="secondary-btn" data-click="free-movelist-load">Load</button>
                  <button class="secondary-btn" data-click="free-movelist-copy">Copy</button>
                </div>
              </div>
            </div>
        `;

        this._content = /** @type {HTMLDivElement} */ (root.querySelector('#content'));
        this._toastEl = /** @type {HTMLDivElement} */ (root.querySelector('#toast'));
        this._settingsOverlay = /** @type {HTMLDivElement} */ (root.querySelector('#settings-overlay'));
        this._settingsBody = /** @type {HTMLDivElement} */ (root.querySelector('#settings-body'));
        this._moveListOverlay = /** @type {HTMLDivElement} */ (root.querySelector('#movelist-overlay'));
        this._moveListText = /** @type {HTMLTextAreaElement} */ (root.querySelector('#movelist-text'));

        root.addEventListener('click', (e) => {
            const target = /** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null);
            if (!target) return;

            const back = target.closest('[data-action="back"]');
            if (back) {
                this._handleBack();
                return;
            }

            const settings = target.closest('[data-action="settings"]');
            if (settings) {
                this._settingsOpen = true;
                this._renderSettings();
                return;
            }
            const close = target.closest('[data-action="close-settings"]');
            if (close) {
                this._settingsOpen = false;
                this._renderSettings();
                return;
            }

            if (this._settingsOpen && target === this._settingsOverlay) {
                this._settingsOpen = false;
                this._renderSettings();
                return;
            }

            const closeMoveList = target.closest('[data-action="close-movelist"]');
            if (closeMoveList) {
                this._moveListOpen = false;
                this._renderMoveListModal();
                return;
            }
            if (this._moveListOpen && target === this._moveListOverlay) {
                this._moveListOpen = false;
                this._renderMoveListModal();
                return;
            }

            const route = this._route;
            const card = target.closest('[data-card]');
            if (route === 'home' && card) {
                const which = card.getAttribute('data-card');
                if (which === 'match') this._setRoute('match-setup');
                if (which === 'free') this._startFree();
                if (which === 'analysis') this._setRoute('analysis-setup');
                return;
            }

            const action = target.closest('[data-click]');
            if (!action) return;
            const click = action.getAttribute('data-click');
            if (!click) return;
            this._dispatchClick(click, action);
        });
    }

    _handleBack() {
        switch (this._route) {
            case 'home':
                return;
            case 'match-setup':
                this._setRoute('home');
                return;
            case 'analysis-setup':
                this._setRoute('home');
                return;
            case 'match':
            case 'free':
            case 'analysis':
            case 'result':
                this._setRoute('home');
                return;
            default:
                this._setRoute('home');
        }
    }

    _setRoute(route) {
        this._route = route;
        this._render();
    }

    // === Rendering ===

    _render() {
        this._renderTopbar();
        this._renderContent();
        this._updateEnginePill();
        this._renderSettings();
        this._renderMoveListModal();
    }

    _renderTopbar() {
        const backBtn = document.querySelector('[data-action="back"]');
        if (backBtn instanceof HTMLElement) {
            backBtn.classList.toggle('hidden', this._route === 'home');
        }
    }

    _renderContent() {
        switch (this._route) {
            case 'home':
                this._content.innerHTML = this._renderHome();
                return;
            case 'match-setup':
                this._content.innerHTML = this._renderMatchSetup();
                this._wireMatchSetup();
                return;
            case 'analysis-setup':
                this._content.innerHTML = this._renderAnalysisSetup();
                this._wireAnalysisSetup();
                return;
            case 'match':
                this._content.innerHTML = this._renderBoardScreen('match');
                this._wireBoard();
                return;
            case 'free':
                this._content.innerHTML = this._renderBoardScreen('free');
                this._wireBoard();
                return;
            case 'analysis':
                this._content.innerHTML = this._renderBoardScreen('analysis');
                this._wireBoard();
                this._wireMoveList('analysis');
                this._wireAnalysisGraph();
                return;
            case 'result':
                this._content.innerHTML = this._renderResult();
                return;
            default:
                this._content.innerHTML = this._renderHome();
        }
    }

    _renderHome() {
        const engineText = this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING';
        return `
	          <div class="page page--home">
	            <div class="card" data-card="match" style="cursor:pointer">
	              <small>AIとの対戦</small>
	              <div class="card-title"><strong>Play vs AI</strong><span style="opacity:.8">🤖</span></div>
	            </div>
            <div class="card" data-card="free" style="cursor:pointer">
              <small>自由モード</small>
              <div class="card-title"><strong>Free Mode</strong><span style="opacity:.8">✎</span></div>
            </div>
            <div class="card" data-card="analysis" style="cursor:pointer">
              <small>分析</small>
              <div class="card-title"><strong>Analysis</strong><span style="opacity:.8">▤</span></div>
            </div>
            <div style="display:flex; justify-content:center; margin: 18px 0 0;">
              <span class="pill" id="engine-pill"><span class="dot ${this._engineReady ? 'ok' : ''}"></span>${engineText}</span>
            </div>
          </div>
        `;
    }

	    _renderMatchSetup() {
	        const tabPresetActive = this._setup.tab === 'preset';
	        const tabManualActive = this._setup.tab === 'manual';
	        return `
	          <div class="page">
	            <div class="card">
	              <div style="display:flex; align-items:center; justify-content:space-between; gap:10px;">
	                <div style="font-weight:650;">AI Match Setup</div>
	                <span class="pill" id="engine-pill"><span class="dot ${this._engineReady ? 'ok' : ''}"></span>${this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING'}</span>
              </div>
              <div style="margin-top: 12px;" class="tabs">
                <button class="tab ${tabPresetActive ? 'active' : ''}" data-click="setup-tab" data-tab="preset">Preset</button>
                <button class="tab ${tabManualActive ? 'active' : ''}" data-click="setup-tab" data-tab="manual">Manual</button>
              </div>
              ${tabPresetActive ? this._renderPresetSetup() : this._renderManualSetup()}
              <div style="margin-top: 14px;">
                <button class="primary-btn" data-click="start-match">START MATCH ▶</button>
              </div>
            </div>
          </div>
        `;
    }

    _renderPresetSetup() {
        const color = this._setup.playerColor;
        const preset = this._setup.preset;
        const presets = [
            { key: 'beginner', name: 'Beginner', depth: 4, desc: 'Learns rules and basic placement.' },
            { key: 'intermediate', name: 'Intermediate', depth: 8, desc: 'Focuses on edge and stability.' },
            { key: 'expert', name: 'Expert', depth: 16, desc: 'Uses parity and mobility.' },
            { key: 'grandmaster', name: 'Grandmaster', depth: 24, desc: 'Near-perfect endgame.' },
        ];

        const list = presets
            .map((p, i) => {
                const active = preset === p.key;
                return `
                  <button class="chip ${active ? 'active' : ''}" data-click="preset" data-preset="${p.key}" style="justify-content:space-between;">
                    <span style="display:flex; flex-direction:column; align-items:flex-start; gap:2px;">
                      <span style="opacity:.8">${String(i + 1).padStart(2, '0')}</span>
                      <span style="font-weight:650">${p.name}</span>
                      <span style="font-size:12px; color: var(--muted); text-align:left">${p.desc}</span>
                    </span>
                    <span class="pill">Depth ${p.depth}</span>
                  </button>
                `;
            })
            .join('');

        return `
          <div class="section">
            <h2>COLOR</h2>
            <div class="row">
              <button class="chip ${color === 'black' ? 'active' : ''}" data-click="player-color" data-color="black">Black</button>
              <button class="chip ${color === 'white' ? 'active' : ''}" data-click="player-color" data-color="white">White</button>
            </div>
          </div>
          <div class="section">
            <h2>SELECT DIFFICULTY</h2>
            <div style="display:flex; flex-direction:column; gap:10px;">${list}</div>
          </div>
        `;
    }

    _renderManualSetup() {
        const openingOptions = [
            `<option value="none">No Opening</option>`,
            ...OPENINGS.map(([idx, name]) => `<option value="${idx}">${escapeHtml(String(name))}</option>`),
        ].join('');

        const color = this._setup.playerColor;
        return `
          <div class="section">
            <h2>COLOR</h2>
            <div class="row">
              <button class="chip ${color === 'black' ? 'active' : ''}" data-click="player-color" data-color="black">Black</button>
              <button class="chip ${color === 'white' ? 'active' : ''}" data-click="player-color" data-color="white">White</button>
            </div>
          </div>
          <div class="section">
            <h2>SELECT OPENING（定石選択）</h2>
            <select class="select" data-click="opening" id="opening-select">${openingOptions}</select>
          </div>
          <div class="section">
            <h2>START FROM CUSTOM POSITION</h2>
            <textarea class="textarea" placeholder="e.g. f5 d6 c3 ... (or f5d6c3)" data-input="move-list"></textarea>
            <div style="margin-top: 10px;">
              <button class="secondary-btn" data-click="apply-move-list">Input Move List（棋譜入力）</button>
            </div>
          </div>
          <div class="section">
            <h2>AI THINKING DEPTH</h2>
            <div class="row" style="justify-content:space-between;">
              <span style="color:var(--muted); font-size:12px;">SEARCH DEPTH</span>
              <span style="font-weight:650;">${this._setup.aiLevel}</span>
            </div>
            <input type="range" min="1" max="24" value="${this._setup.aiLevel}" data-input="manual-depth" style="width:100%;" />
          </div>
        `;
    }

    _wireMatchSetup() {
        const openingSelect = this._content.querySelector('#opening-select');
        if (openingSelect instanceof HTMLSelectElement) {
            openingSelect.value =
                this._setup.humanOpening === null || this._setup.humanOpening === undefined
                    ? 'none'
                    : String(this._setup.humanOpening);
            openingSelect.addEventListener('change', () => {
                const v = openingSelect.value;
                this._setup.humanOpening = v === 'none' ? null : Number(v);
            });
        }

        const moveList = this._content.querySelector('[data-input="move-list"]');
        if (moveList instanceof HTMLTextAreaElement) {
            moveList.value = this._setup.moveList ?? '';
            moveList.addEventListener('input', () => {
                this._setup.moveList = moveList.value;
            });
        }

        const manualDepth = this._content.querySelector('[data-input="manual-depth"]');
        if (manualDepth instanceof HTMLInputElement) {
            manualDepth.addEventListener('input', () => {
                this._setup.aiLevel = clampInt(Number(manualDepth.value), 1, 24);
                this._render();
            });
        }
    }

	    _renderAnalysisSetup() {
	        const level = this._analysis.aiLevel;
	        const engineText = this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING';
	        const busy = this._analysis.isAnalyzing;
	        return `
	          <div class="page">
	            <div class="card">
	              <div style="display:flex; align-items:center; justify-content:space-between; gap:10px;">
	                <div style="font-weight:650;">Analysis Setup</div>
	                <span class="pill" id="engine-pill"><span class="dot ${this._engineReady ? 'ok' : ''}"></span>${engineText}</span>
              </div>
              <div class="section">
                <h2>MOVE LIST</h2>
                <textarea class="textarea" data-input="analysis-setup-move-list" placeholder="e.g. f5 d6 c3 ... (or f5d6c3)">${escapeHtml(
                    this._analysis.moveList
                )}</textarea>
              </div>
              <div class="section">
                <h2>AI LEVEL (ANALYSIS)</h2>
                <div class="row" style="justify-content:space-between;">
                  <span style="color:var(--muted); font-size:12px;">DEPTH</span>
                  <span style="font-weight:650;">${level}</span>
                </div>
                <input type="range" min="1" max="24" value="${level}" data-input="analysis-setup-level" style="width:100%;" />
              </div>
              ${
                  busy
                      ? `<div class="section"><h2>STATUS</h2><div style="color:var(--muted); font-size:12px;">Analyzing...</div></div>`
                      : ''
              }
              <div style="margin-top: 14px;">
                <button class="primary-btn" data-click="start-analysis" ${busy ? 'disabled' : ''}>${busy ? 'ANALYZING...' : 'ANALYZE ▶'}</button>
              </div>
            </div>
          </div>
        `;
    }

    _wireAnalysisSetup() {
        const area = this._content.querySelector('[data-input="analysis-setup-move-list"]');
        if (area instanceof HTMLTextAreaElement) {
            area.addEventListener('input', () => {
                this._analysis.moveList = area.value;
            });
        }
        const slider = this._content.querySelector('[data-input="analysis-setup-level"]');
        if (slider instanceof HTMLInputElement) {
            slider.addEventListener('input', () => {
                const v = clampInt(Number(slider.value), 1, 24);
                this._analysis.aiLevel = v;
                this._analysis.hintDepth = v;
                this._render();
            });
        }
    }

	    _renderBoardScreen(mode) {
        const vm = this._viewModel;
        const black = vm?.blackBits ?? null;
        const white = vm?.whiteBits ?? null;
        const blackCount = Board.countBits(black);
        const whiteCount = Board.countBits(white);
        const next = vm?.nextTurn ?? Turn.BLACK;
        const ply = (this.gameService.record ?? []).length;

        const subtitle = (() => {
            if (mode === 'match') return 'Match';
            if (mode === 'free') return 'Free Mode';
            return 'Analysis';
        })();

        const opening = vm?.currentHumanOpening ? `定石: ${escapeHtml(vm.currentHumanOpening)}` : '';
        const hintEnabled = this.gameService.hintEnabled;
        const record = this.gameService.record ?? [];
        const recordText = record.join(' ');

        const boardHtml = this._renderBoard(vm);
        const controlsHtml = this._renderControls(mode, hintEnabled);

        const analysisOverlay =
            mode !== 'analysis'
                ? ''
                : `
                    <div class="board-overlay">
                      <div class="toggle ${this._analysis.mode === 'edit' ? 'on' : ''}" data-click="analysis-toggle" role="switch" aria-checked="${this._analysis.mode === 'edit' ? 'true' : 'false'}">
                        <span class="mini">${this._analysis.mode === 'edit' ? 'EDIT' : 'PLAY'}</span>
                        <span class="track"><span class="knob"></span></span>
                      </div>
                    </div>
                  `;

        const analysisTabs =
            mode !== 'analysis'
                ? ''
                : `
                    <div class="tabs-row">
                      <button class="${this._analysis.tab === 'graph' ? 'active' : ''}" data-click="analysis-tab" data-tab="graph">GRAPH</button>
                      <button class="${this._analysis.tab === 'moveList' ? 'active' : ''}" data-click="analysis-tab" data-tab="moveList">MOVE LIST</button>
                    </div>
                  `;

        const analysisPane =
            mode !== 'analysis'
                ? ''
                : this._analysis.tab === 'graph'
                    ? this._renderAnalysisGraph()
                    : this._renderAnalysisMoveList(recordText);

	        const analysisToolbar =
	            mode === 'analysis' ? this._renderAnalysisToolbar() : mode === 'free' ? this._renderFreeToolbar() : '';

        return `
	          <div class="page page--board">
	            <div class="card">
	              <div style="display:flex; align-items:center; justify-content:space-between;">
	                <div style="display:flex; flex-direction:column; gap:2px;">
	                  <div style="font-weight:650;">${subtitle}</div>
                  <div style="font-size:12px; color: var(--muted);">${opening}</div>
                </div>
                <span class="pill" id="engine-pill"><span class="dot ${this._engineReady ? 'ok' : ''}"></span>${this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING'}</span>
              </div>
              <div class="section" style="margin-top:12px;">
                <h2>SCORE</h2>
                <div class="score-row">
                  <div class="score-side score-left"><span style="opacity:.85;">●</span> ${escapeHtml(this._blackPlayerName || 'Black')} <span style="font-weight:650; margin-left:8px;">${blackCount}</span></div>
                  <div class="score-center">TURN: ${next === Turn.BLACK ? 'BLACK' : 'WHITE'} / MOVE: ${ply}</div>
                  <div class="score-side score-right"><span style="opacity:.85;">○</span> ${escapeHtml(this._whitePlayerName || 'White')} <span style="font-weight:650; margin-left:8px;">${whiteCount}</span></div>
                </div>
              </div>
              <div class="board-wrap ${mode === 'analysis' ? 'with-overlay' : ''}">${analysisOverlay}${boardHtml}</div>
              ${analysisToolbar}
              ${analysisTabs}
              ${controlsHtml}
              ${analysisPane}
            </div>
          </div>
        `;
    }

    _renderMoveListModal() {
        if (!this._moveListOverlay || !this._moveListText) return;
        this._moveListOverlay.classList.toggle('open', this._moveListOpen);
        if (this._moveListOpen) {
            this._moveListText.value = this._free.moveList ?? (this.gameService.record ?? []).join(' ');
        }
    }

    _renderAnalysisToolbar() {
        const total = this._analysis.analysis?.record?.length ?? 0;
        const ply = this._analysis.mode === 'play' ? this._analysis.cursorPly ?? 0 : this._analysis.scenarioPly ?? 0;
        const canFirst = this._analysis.mode === 'play' ? ply > 0 : ply > this._analysis.scenarioBasePly;
        const canPrev = this._analysis.mode === 'play' ? ply > 0 : this.gameService.canUndo;
        const canNext = this._analysis.mode === 'play' ? ply < total : this.gameService.canRedo;
        const canLast = this._analysis.mode === 'play' ? ply < total : this.gameService.canRedo;
        const hintOn = this.gameService.hintEnabled;
        return `
          <div class="analysis-toolbar">
            <div class="analysis-toolbar-left">
              <button class="icon-btn mini-icon-btn controls-icon" data-click="analysis-first" aria-label="first" ${canFirst ? '' : 'disabled'}>&lt;&lt;</button>
              <button class="icon-btn mini-icon-btn controls-icon" data-click="analysis-prev" aria-label="prev" ${canPrev ? '' : 'disabled'}>&lt;</button>
              <button class="icon-btn mini-icon-btn controls-icon" data-click="analysis-next" aria-label="next" ${canNext ? '' : 'disabled'}>&gt;</button>
              <button class="icon-btn mini-icon-btn controls-icon" data-click="analysis-last" aria-label="last" ${canLast ? '' : 'disabled'}>&gt;&gt;</button>
              <div class="toggle ${hintOn ? 'on' : ''}" data-click="hint-toggle" role="switch" aria-checked="${hintOn ? 'true' : 'false'}">
                <span class="mini">HINT</span>
                <span class="track"><span class="knob"></span></span>
              </div>
              <button class="controls-btn" data-click="analysis-ai-move">AI Move</button>
            </div>
          </div>
        `;
    }

    _renderFreeToolbar() {
        const canUndo = this.gameService.canUndo;
        const canRedo = this.gameService.canRedo;
        const hintOn = this.gameService.hintEnabled;
        return `
          <div class="analysis-toolbar">
            <div class="toolbar-stack">
              <div class="toolbar-row">
                <button class="icon-btn mini-icon-btn controls-icon" data-click="free-first" aria-label="first" ${canUndo ? '' : 'disabled'}>&lt;&lt;</button>
                <button class="icon-btn mini-icon-btn controls-icon" data-click="free-prev" aria-label="prev" ${canUndo ? '' : 'disabled'}>&lt;</button>
                <button class="icon-btn mini-icon-btn controls-icon" data-click="free-next" aria-label="next" ${canRedo ? '' : 'disabled'}>&gt;</button>
                <button class="icon-btn mini-icon-btn controls-icon" data-click="free-last" aria-label="last" ${canRedo ? '' : 'disabled'}>&gt;&gt;</button>
              </div>
              <div class="toolbar-row">
                <div class="toggle ${hintOn ? 'on' : ''}" data-click="hint-toggle" role="switch" aria-checked="${hintOn ? 'true' : 'false'}">
                  <span class="mini">HINT</span>
                  <span class="track"><span class="knob"></span></span>
                </div>
                <button class="controls-btn" data-click="free-ai-move">AI Move</button>
                <button class="controls-btn" data-click="free-movelist-open">Move List</button>
              </div>
            </div>
          </div>
        `;
    }

    // free mode move list UI is handled via modal

    _renderAnalysisMoveList(recordText) {
        const record = this._analysis.analysis?.record ?? this.gameService.record ?? [];
        const a = this._analysis.analysis?.analysis ?? null;
        const cursor = this._analysis.mode === 'play' ? this._analysis.cursorPly ?? 0 : this._analysis.scenarioPly ?? 0;

        const items = record
            .map((m, i) => {
                const plyAfter = i + 1;
                const n = i + 1;
                const evalAfter = a && a[plyAfter] ? a[plyAfter].evalBlack : null;
                const bestBefore = a && a[i] ? a[i].bestMove : null;
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

    _renderAnalysisGraph() {
        const record = this._analysis.analysis?.record ?? this.gameService.record ?? [];
        const series = this._analysis.analysis?.analysis
            ? this._analysis.analysis.analysis.map((x) => x.evalBlack)
            : computeMaterialDiffSeries(record);
        const cursor = this._analysis.cursorPly ?? 0;
        const hover = this._analysis.hoverPly;
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

    _renderControls(mode, hintEnabled) {
        const hintLabel = hintEnabled ? 'Hint: ON' : 'Hint: OFF';
        const aiLevel = mode === 'analysis' ? this._analysis.aiLevel : this._free.aiLevel;

        if (mode === 'analysis' || mode === 'free') {
            // 分析/Free はツールバー側へ集約（<< < > >> / Hint / AI / Move list）
            return '';
        }

        const hintControls =
            mode === 'match'
                ? ''
                : `
                    <div class="row">
                      <button class="chip ${hintEnabled ? 'active' : ''}" data-click="hint-toggle">${hintLabel}</button>
                      <button class="chip" data-click="ai-move-once">AI Move (Lv ${aiLevel})</button>
                    </div>
                `;

        return `
          <div class="section">
            <h2>CONTROLS</h2>
            <div class="row">
              <button class="chip" data-click="undo">Undo</button>
              <button class="chip" data-click="redo">Redo</button>
              ${mode === 'match' ? `<button class="chip" data-click="back-to-setup">Setup</button>` : ''}
            </div>
            ${hintControls}
          </div>
        `;
    }

    _renderBoard(vm) {
        const legalBits = vm?.legalMovesBits ?? 0n;
        const blackBits = vm?.blackBits ?? 0n;
        const whiteBits = vm?.whiteBits ?? 0n;
        const lastMove = vm?.lastMove ?? null;
        const nextOpeningPos = vm?.humanOpeningNextPosition ?? null;
        const scores = vm?.eval ?? null;

        let maxScore = null;
        if (scores) {
            for (let i = 0; i < 64; i++) {
                if (((legalBits >> BigInt(i)) & 1n) === 0n) continue;
                const v = scores[i];
                if (v === null || v === undefined) continue;
                if (maxScore === null || v > maxScore) maxScore = v;
            }
        }

        const letters = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];
        const numbers = ['1', '2', '3', '4', '5', '6', '7', '8'];

        const tiles = [];
        for (let row = 0; row < 10; row++) {
            for (let col = 0; col < 10; col++) {
                // corners
                if ((row === 0 || row === 9) && (col === 0 || col === 9)) {
                    tiles.push(`<div class="coord"></div>`);
                    continue;
                }

                // top/bottom letters
                if ((row === 0 || row === 9) && col >= 1 && col <= 8) {
                    tiles.push(`<div class="coord">${letters[col - 1]}</div>`);
                    continue;
                }

                // left/right numbers
                if ((col === 0 || col === 9) && row >= 1 && row <= 8) {
                    tiles.push(`<div class="coord">${numbers[row - 1]}</div>`);
                    continue;
                }

                // inner board cell
                const x = col - 1;
                const y = row - 1;
                const pos = y * 8 + x;

                const isLegal = ((legalBits >> BigInt(pos)) & 1n) === 1n;
                const isBlack = ((blackBits >> BigInt(pos)) & 1n) === 1n;
                const isWhite = ((whiteBits >> BigInt(pos)) & 1n) === 1n;
                const isLast = lastMove === pos;
                const isNextOpening = nextOpeningPos === pos;
                const score = scores ? scores[pos] : null;
                const showScore = score !== null && score !== undefined && isLegal;
                const scoreColor =
                    maxScore !== null && score === maxScore ? 'style="color: var(--score-best)"' : '';

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

                tiles.push(
                    `<button class="${classes}" data-click="cell" data-pos="${pos}" aria-label="cell ${pos}">${stone}${scoreHtml}</button>`
                );
            }
        }

        return `<div class="board-shell" role="grid">${tiles.join('')}</div>`;
    }

	    _renderResult() {
        const r = this._result;
        if (!r) return this._renderHome();

	        const winner = r.blackScore === r.whiteScore ? 'DRAW' : r.blackScore > r.whiteScore ? 'BLACK' : 'WHITE';
	        return `
	          <div class="page">
	            <div class="card">
	              <div style="display:flex; align-items:center; justify-content:space-between;">
	                <div style="font-weight:650;">Result</div>
	                <span class="pill" id="engine-pill"><span class="dot ${this._engineReady ? 'ok' : ''}"></span>${this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING'}</span>
              </div>
              <div class="section">
                <h2>SCORE</h2>
                <div class="row" style="justify-content:space-between;">
                  <div><span style="opacity:.85;">●</span> ${escapeHtml(r.blackPlayerName)} <span style="font-weight:650; margin-left:8px;">${r.blackScore}</span></div>
                  <div><span style="opacity:.85;">○</span> ${escapeHtml(r.whitePlayerName)} <span style="font-weight:650; margin-left:8px;">${r.whiteScore}</span></div>
                </div>
                <div style="margin-top:10px; color:var(--muted);">WINNER: <span style="color:var(--text); font-weight:650;">${winner}</span></div>
              </div>
              <div class="section">
                <h2>MATCH RECORD</h2>
                <textarea class="textarea" readonly>${escapeHtml(this._resultRecordText)}</textarea>
                <div class="row" style="margin-top:10px;">
                  <button class="secondary-btn" data-click="copy-result">Copy</button>
                  <button class="secondary-btn" data-click="go-analysis">Go to Analysis</button>
                </div>
              </div>
              <div style="margin-top: 14px;">
                <button class="primary-btn" data-click="back-home">HOME</button>
              </div>
            </div>
          </div>
        `;
    }

    _updateEnginePill() {
        for (const pill of document.querySelectorAll('#engine-pill')) {
            if (!(pill instanceof HTMLElement)) continue;
            pill.innerHTML = `<span class="dot ${this._engineReady ? 'ok' : ''}"></span>${this._engineReady ? 'ENGINE READY' : 'ENGINE LOADING'}`;
        }
    }

    // === Event wiring ===

    _wireBoard() {
        const depth = this._content.querySelector('[data-input="hint-depth"]');
        if (depth instanceof HTMLInputElement) {
            depth.addEventListener('input', () => {
                const v = clampInt(Number(depth.value), 1, 24);
                if (this._route === 'analysis') this._analysis.hintDepth = v;
                if (this._route === 'free') this._free.hintDepth = v;
                this.gameService.setHintLevel(v);
                this._render();
            });
        }
    }

    _wireMoveList(mode) {
        const input = this._content.querySelector(`[data-input="${mode}-move-list"]`);
        if (!(input instanceof HTMLTextAreaElement)) return;
        input.addEventListener('input', () => {
            if (mode === 'analysis') this._analysis.moveList = input.value;
            if (mode === 'free') this._free.moveList = input.value;
        });
    }

    _dispatchClick(click, el) {
        switch (click) {
            case 'start-analysis': {
                this._startAnalysisJob();
                return;
            }
            case 'analysis-tab': {
                const tab = el.getAttribute('data-tab');
                if (tab === 'graph' || tab === 'moveList') {
                    this._analysis.tab = tab;
                    this._render();
                }
                return;
            }
            case 'analysis-toggle': {
                this._setAnalysisMode(this._analysis.mode === 'edit' ? 'play' : 'edit');
                return;
            }
            case 'analysis-first': {
                if (this._analysis.mode === 'play') {
                    this._jumpToPly(0);
                } else {
                    this._resetScenarioToBase();
                }
                return;
            }
            case 'analysis-prev': {
                this._analysisStep(-1);
                return;
            }
            case 'analysis-next': {
                this._analysisStep(1);
                return;
            }
            case 'analysis-last': {
                if (this._analysis.mode === 'play') {
                    const total = this._analysis.analysis?.record?.length ?? 0;
                    this._jumpToPly(total);
                } else {
                    this.gameService.redoToEnd();
                }
                return;
            }
            case 'analysis-ai-move': {
                // 分析でのAI着手は分岐（if検証）なので Edit に寄せる
                if (this._analysis.mode !== 'edit') {
                    this._setAnalysisMode('edit');
                }
                this.gameService.playAiMoveOnce(this._analysis.aiLevel);
                return;
            }
            case 'free-first':
                this.gameService.undoToStart();
                return;
            case 'free-prev':
                this.gameService.undo();
                return;
            case 'free-next':
                this.gameService.redo();
                return;
            case 'free-last':
                this.gameService.redoToEnd();
                return;
            case 'free-ai-move':
                this.gameService.playAiMoveOnce(this._free.aiLevel);
                return;
            case 'free-movelist-open':
                this._moveListOpen = true;
                this._renderMoveListModal();
                return;
            case 'free-movelist-load': {
                if (!this._moveListText) return;
                const parsed = this.gameService.parseMoveList(this._moveListText.value ?? '');
                if (!parsed.ok) {
                    this._toast(parsed.error, true);
                    return;
                }
                this._free.moveList = this._moveListText.value ?? '';
                this.gameService.startSession({
                    aiEnabled: false,
                    aiLevel: this._free.aiLevel,
                    aiTurn: this._initialSettings.aiTurn ?? 'white',
                    humanOpening: null,
                    blackPlayerName: 'Black',
                    whitePlayerName: 'White',
                    record: parsed.record,
                    enableHint: this.gameService.hintEnabled,
                    hintLevel: this._free.hintDepth,
                    resetEngine: false,
                });
                this._moveListOpen = false;
                this._renderMoveListModal();
                return;
            }
            case 'free-movelist-copy':
                this._copyText(this._moveListText?.value ?? '');
                return;
            case 'setup-tab': {
                const tab = el.getAttribute('data-tab');
                if (tab === 'preset' || tab === 'manual') {
                    this._setup.tab = tab;
                    this._render();
                }
                return;
            }
            case 'player-color': {
                const c = el.getAttribute('data-color');
                if (c === 'black' || c === 'white') {
                    this._setup.playerColor = c;
                    this._render();
                }
                return;
            }
            case 'preset': {
                const p = el.getAttribute('data-preset');
                if (!p) return;
                this._setup.preset = p;
                this._setup.aiLevel = presetToLevel(p);
                this._render();
                return;
            }
            case 'apply-move-list': {
                const area = this._content.querySelector('[data-input="move-list"]');
                if (area instanceof HTMLTextAreaElement) {
                    this._setup.moveList = area.value;
                    this._toast('Move list saved', false);
                }
                return;
            }
            case 'start-match': {
                this._startMatch();
                return;
            }
            case 'cell': {
                if (this._route !== 'match' && this._route !== 'free' && this._route !== 'analysis') return;
                const pos = Number(el.getAttribute('data-pos'));
                if (!Number.isFinite(pos)) return;
                if (this._route === 'analysis' && this._analysis.mode !== 'edit') {
                    // Playモードで盤面をクリックしたら、Edit（if検証）へ自動遷移して着手する
                    this._setAnalysisMode('edit');
                }
                this.gameService.handleBoardClick(pos);
                return;
            }
            case 'undo':
                this.gameService.undo();
                return;
            case 'redo':
                this.gameService.redo();
                return;
            case 'back-to-setup':
                this._setRoute('match-setup');
                return;
            case 'hint-toggle':
                this.gameService.toggleHint();
                return;
            case 'hint-refresh': {
                const level = this.gameService.hintLevel;
                this.gameService.requestDeepHint(level);
                this._toast('Hint refreshing...', false);
                return;
            }
            case 'ai-move-once': {
                const lv = this._route === 'analysis' ? this._analysis.aiLevel : this._free.aiLevel;
                this.gameService.playAiMoveOnce(lv);
                return;
            }
            case 'free-load':
                // deprecated
                return;
            case 'analysis-load':
                this._startAnalysisJob();
                return;
            case 'free-copy':
                this._copyText((this.gameService.record ?? []).join(' '));
                return;
            case 'analysis-copy':
                this._copyText((this._analysis.analysis?.record ?? this.gameService.record ?? []).join(' '));
                return;
            case 'seek-ply': {
                const ply = clampInt(Number(el.getAttribute('data-ply')), 0, 10_000);
                this._jumpToPly(ply);
                return;
            }
            case 'copy-result':
                this._copyText(this._resultRecordText);
                return;
            case 'go-analysis': {
                this._analysis.moveList = this._resultRecordText;
                this._setRoute('analysis-setup');
                return;
            }
            case 'back-home':
                this._setRoute('home');
                return;
            default:
                return;
        }
    }

    _startMatch() {
        const aiLevel =
            this._setup.tab === 'preset' ? presetToLevel(this._setup.preset) : clampInt(this._setup.aiLevel, 1, 24);

        const playerColor = this._setup.playerColor;
        const aiTurn = playerColor === 'black' ? 'white' : 'black';

        const blackPlayerName = aiTurn === 'black' ? `AI Lv ${aiLevel}` : 'あなた';
        const whitePlayerName = aiTurn === 'white' ? `AI Lv ${aiLevel}` : 'あなた';

        const record = (() => {
            if (this._setup.tab !== 'manual') return null;
            const parsed = this.gameService.parseMoveList(this._setup.moveList ?? '');
            if (!parsed.ok) {
                this._toast(parsed.error, true);
                return null;
            }
            return parsed.record;
        })();

        this.gameService.startSession({
            aiEnabled: true,
            aiLevel,
            aiTurn,
            humanOpening:
                this._setup.tab === 'manual'
                    ? this._setup.humanOpening ?? null
                    : this._initialSettings.humanOpening ?? null,
            blackPlayerName,
            whitePlayerName,
            record,
            enableHint: false,
            resetEngine: false,
        });

        this._setRoute('match');
    }

    _startFree() {
        const blackPlayerName = 'Black';
        const whitePlayerName = 'White';
        this._free.aiLevel = clampInt(this._initialSettings.aiLevel ?? 8, 1, 24);
        this._free.hintDepth = clampInt(this.gameService.hintLevel ?? 8, 1, 24);
        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: this._free.aiLevel,
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName,
            whitePlayerName,
            record: null,
            enableHint: false,
            hintLevel: this._free.hintDepth,
            resetEngine: false,
        });
        this._setRoute('free');
    }

    _startAnalysis(fromResult = false) {
        const blackPlayerName = 'Black';
        const whitePlayerName = 'White';

        const recordText = fromResult ? this._analysis.moveList : this._analysis.moveList;
        const parsed = this.gameService.parseMoveList(recordText ?? '');
        const record = parsed.ok ? parsed.record : null;
        if (!parsed.ok && recordText?.trim()) {
            this._toast(parsed.error, true);
        }

        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: this._analysis.aiLevel,
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName,
            whitePlayerName,
            record,
            enableHint: this.gameService.hintEnabled,
            hintLevel: this._analysis.hintDepth,
            resetEngine: false,
        });
        this._setRoute('analysis');
    }

    _loadMoveList(mode) {
        const text = mode === 'analysis' ? this._analysis.moveList : this._free.moveList;
        const parsed = this.gameService.parseMoveList(text ?? '');
        if (!parsed.ok) {
            this._toast(parsed.error, true);
            return;
        }
        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: clampInt(this._initialSettings.aiLevel ?? 8, 1, 24),
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName: 'Black',
            whitePlayerName: 'White',
            record: parsed.record,
            enableHint: this.gameService.hintEnabled,
            hintLevel: mode === 'analysis' ? this._analysis.hintDepth : this._free.hintDepth,
            resetEngine: false,
        });
        this._toast('Loaded', false);
    }

    _wireAnalysisGraph() {
        if (this._route !== 'analysis') return;
        const graph = this._content.querySelector('.graph');
        if (!(graph instanceof HTMLElement)) return;

        graph.onmousemove = (ev) => {
            const a = this._analysis.analysis?.analysis;
            const n = a ? a.length - 1 : (this._analysis.analysis?.record?.length ?? 0);
            if (n <= 0) return;
            const rect = graph.getBoundingClientRect();
            const x = ev.clientX - rect.left;
            const t = Math.max(0, Math.min(1, x / rect.width));
            const ply = Math.round(t * n);
            if (this._analysis.hoverPly !== ply) {
                this._analysis.hoverPly = ply;
                this._render();
            }
        };
        graph.onmouseleave = () => {
            if (this._analysis.hoverPly !== null) {
                this._analysis.hoverPly = null;
                this._render();
            }
        };

        graph.onclick = (ev) => {
            const target = ev.target instanceof Element ? ev.target : null;
            const plyAttr = target?.getAttribute?.('data-ply');
            if (plyAttr) {
                this._jumpToPly(clampInt(Number(plyAttr), 0, 10_000));
                return;
            }

            const a = this._analysis.analysis?.analysis;
            const n = a ? a.length - 1 : (this._analysis.analysis?.record?.length ?? 0);
            if (n <= 0) return;
            const rect = graph.getBoundingClientRect();
            const x = ev.clientX - rect.left;
            const t = Math.max(0, Math.min(1, x / rect.width));
            const ply = Math.round(t * n);
            this._jumpToPly(ply);
        };
    }

    _analysisStep(delta) {
        if (this._route !== 'analysis' || !this._analysis.analysis?.record) return;
        if (this._analysis.mode === 'play') {
            this._jumpToPly((this._analysis.cursorPly ?? 0) + delta);
            return;
        }
        // edit: scenario undo/redo
        if (delta < 0) this.gameService.undo();
        if (delta > 0) this.gameService.redo();
    }

    _resetScenarioToBase() {
        const base = this._analysis.scenarioBasePly ?? 0;
        const full = this._analysis.analysis?.record ?? [];
        const prefix = full.slice(0, clampInt(base, 0, full.length));
        this._analysis.mode = 'edit';
        this._analysis.scenarioPly = prefix.length;
        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: this._analysis.aiLevel,
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName: 'Black',
            whitePlayerName: 'White',
            record: prefix,
            enableHint: this.gameService.hintEnabled,
            hintLevel: this._analysis.hintDepth,
            truncateHistoryToCurrent: true,
            resetEngine: false,
        });
    }

    _jumpToPly(ply) {
        const full = this._analysis.analysis?.record;
        if (!full) return;
        const p = clampInt(ply, 0, full.length);
        // record側のジャンプはPlayを基本とし、分岐編集はここでは行わない
        this._analysis.cursorPly = p;
        this._analysis.scenarioPly = p;
        this._analysis.mode = 'play';
        this._analysis.hoverPly = null;
        this._analysis.scenarioBasePly = p;

        const prefix = full.slice(0, p);
        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: this._analysis.aiLevel,
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName: 'Black',
            whitePlayerName: 'White',
            record: prefix,
            enableHint: this.gameService.hintEnabled,
            hintLevel: this._analysis.hintDepth,
            resetEngine: false,
        });
        if (this._analysis.mode === 'edit') {
            // クリックでply移動したら、その局面からif検証をやり直す
            this._analysis.mode = 'edit';
        }
        this._render();
    }

    _setAnalysisMode(mode) {
        if (this._analysis.mode === mode) return;
        this._analysis.mode = mode;
        this._analysis.hoverPly = null;
        if (this._route !== 'analysis') {
            this._render();
            return;
        }
        if (mode === 'edit') {
            this._analysis.scenarioBasePly = this._analysis.cursorPly ?? 0;
            this._analysis.scenarioPly = this._analysis.scenarioBasePly;
            // edit開始時は基点局面へ移動し、Undo/Redoを分岐内に限定する
            const full = this._analysis.analysis?.record ?? [];
            const prefix = full.slice(0, clampInt(this._analysis.scenarioBasePly, 0, full.length));
            this.gameService.startSession({
                aiEnabled: false,
                aiLevel: this._analysis.aiLevel,
                aiTurn: this._initialSettings.aiTurn ?? 'white',
                humanOpening: null,
                blackPlayerName: 'Black',
                whitePlayerName: 'White',
                record: prefix,
                enableHint: this.gameService.hintEnabled,
                hintLevel: this._analysis.hintDepth,
                truncateHistoryToCurrent: true,
                resetEngine: false,
            });
        } else {
            // playに戻すときは棋譜の手順へ戻す
            this._jumpToPly(this._analysis.cursorPly ?? 0);
        }
        this._render();
    }

    _resetAnalysisToCursor() {
        const full = this._analysis.analysis?.record;
        if (!full) return;
        const prefix = full.slice(0, clampInt(this._analysis.cursorPly ?? 0, 0, full.length));
        this.gameService.startSession({
            aiEnabled: false,
            aiLevel: this._analysis.aiLevel,
            aiTurn: this._initialSettings.aiTurn ?? 'white',
            humanOpening: null,
            blackPlayerName: 'Black',
            whitePlayerName: 'White',
            record: prefix,
            enableHint: this.gameService.hintEnabled,
            hintLevel: this._analysis.hintDepth,
            resetEngine: false,
        });
        this._analysis.scenarioPly = prefix.length;
    }

    _startAnalysisJob() {
        if (this._analysis.isAnalyzing) return;
        const parsed = this.gameService.parseMoveList(this._analysis.moveList ?? '');
        if (!parsed.ok) {
            this._toast(parsed.error, true);
            return;
        }
        if (!this._engineReady) {
            this._toast('Engine not ready', true);
            return;
        }

        this._analysis.isAnalyzing = true;
        this._analysis.analysis = null;
        this._render();

        (async () => {
            const level = this._analysis.aiLevel;
            const res = await this.gameService.analyzeRecord(parsed.record, level, {
                onProgress: ({ ply, total }) => {
                    if (ply % 4 === 0 || ply === total) {
                        this._toast(`Analyzing ${ply}/${total}`, false);
                    }
                },
            });

            this._analysis.isAnalyzing = false;

            if (!res.ok) {
                this._toast(res.error, true);
                this._render();
                return;
            }

            this._analysis.analysis = { record: parsed.record, analysis: res.analysis };
            this._analysis.cursorPly = 0;
            this._analysis.hoverPly = null;
            this._analysis.mode = 'play';

            this.gameService.startSession({
                aiEnabled: false,
                aiLevel: this._analysis.aiLevel,
                aiTurn: this._initialSettings.aiTurn ?? 'white',
                humanOpening: null,
                blackPlayerName: 'Black',
                whitePlayerName: 'White',
                record: [],
                enableHint: true,
                hintLevel: this._analysis.hintDepth,
                resetEngine: false,
            });

            this._setRoute('analysis');
        })();
    }

    _renderSettings() {
        if (!this._settingsOverlay || !this._settingsBody) return;
        this._settingsOverlay.classList.toggle('open', this._settingsOpen);
        if (!this._settingsOpen) return;

        const route = this._route;
        const isFreeOrAnalysis = route === 'free' || route === 'analysis';
        const hintEnabled = this.gameService.hintEnabled;
        const hintLevel = this.gameService.hintLevel;

        const aiLevel =
            route === 'analysis'
                ? this._analysis.aiLevel
                : route === 'free'
                    ? this._free.aiLevel
                    : clampInt(this._initialSettings.aiLevel ?? 8, 1, 24);

        this._settingsBody.innerHTML = `
          <div style="color:var(--muted); font-size:12px;">mode: ${escapeHtml(route)}</div>
          <div class="divider"></div>
          ${
              isFreeOrAnalysis
                  ? `
                    <div class="section" style="margin-top:0;">
                      <h2>AI LEVEL</h2>
                      <div class="row" style="justify-content:space-between;">
                        <span style="color:var(--muted); font-size:12px;">AI Move</span>
                        <span style="font-weight:650;">${aiLevel}</span>
                      </div>
                      <input type="range" min="1" max="24" value="${aiLevel}" data-input="settings-ai-level" style="width:100%;" />
                    </div>
                    <div class="section">
                      <h2>HINT</h2>
                      <div class="row">
                        <button class="chip ${hintEnabled ? 'active' : ''}" data-click="hint-toggle">${hintEnabled ? 'Hint: ON' : 'Hint: OFF'}</button>
                        <button class="chip" data-click="hint-refresh">Refresh</button>
                      </div>
                      <div class="row" style="justify-content:space-between;">
                        <span style="color:var(--muted); font-size:12px;">DEPTH</span>
                        <span style="font-weight:650;">${hintLevel}</span>
                      </div>
                      <input type="range" min="1" max="24" value="${hintLevel}" data-input="settings-hint-level" style="width:100%;" />
                    </div>
                  `
                  : `<div style="color:var(--muted); font-size:12px;">No settings for this screen.</div>`
          }
        `;

        const aiSlider = this._settingsBody.querySelector('[data-input="settings-ai-level"]');
        if (aiSlider instanceof HTMLInputElement) {
            aiSlider.addEventListener('input', () => {
                const v = clampInt(Number(aiSlider.value), 1, 24);
                if (this._route === 'analysis') this._analysis.aiLevel = v;
                if (this._route === 'free') this._free.aiLevel = v;
                this._renderSettings();
            });
        }

        const hintSlider = this._settingsBody.querySelector('[data-input="settings-hint-level"]');
        if (hintSlider instanceof HTMLInputElement) {
            hintSlider.addEventListener('input', () => {
                const v = clampInt(Number(hintSlider.value), 1, 24);
                this.gameService.setHintLevel(v);
                if (this._route === 'analysis') this._analysis.hintDepth = v;
                if (this._route === 'free') this._free.hintDepth = v;
                this._renderSettings();
            });
        }
    }

    // === Toast / Clipboard ===

    _toast(message, isError) {
        if (!this._toastEl) return;
        this._toastEl.textContent = message;
        this._toastEl.classList.toggle('error', Boolean(isError));
        this._toastEl.style.display = 'block';
        clearTimeout(this._toastTimer);
        this._toastTimer = setTimeout(() => {
            if (this._toastEl) this._toastEl.style.display = 'none';
        }, 2400);
    }

    async _copyText(text) {
        try {
            await navigator.clipboard.writeText(text ?? '');
            this._toast('Copied', false);
        } catch {
            this._toast('Copy failed', true);
        }
    }
}

function presetToLevel(key) {
    switch (key) {
        case 'beginner':
            return 4;
        case 'intermediate':
            return 8;
        case 'expert':
            return 16;
        case 'grandmaster':
            return 24;
        default:
            return 8;
    }
}

function clampInt(n, min, max) {
    const x = Number.isFinite(n) ? Math.trunc(n) : min;
    return Math.min(max, Math.max(min, x));
}

function escapeHtml(s) {
    return String(s)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

function formatSigned(n) {
    const x = Number(n);
    if (!Number.isFinite(x)) return '0';
    return x > 0 ? `+${x}` : `${x}`;
}

/**
 * 石数差（black - white）を手順ごとに計算する
 * @param {ReadonlyArray<string>} record e.g. ["f5","d6","pass",...]
 * @returns {number[]} series
 */
function computeMaterialDiffSeries(record) {
    try {
        let board = Board.initial();
        /** @type {number[]} */
        const series = [];
        series.push(board.blackCount - board.whiteCount);
        for (const m of record) {
            const t = String(m).toLowerCase();
            if (t === 'pass') {
                board = board.applyPass();
            } else {
                const pos = notationToPositionSafe(t);
                if (pos === null) break;
                if (board.canPlace(pos)) {
                    board = board.applyMove(pos);
                } else if (board.mustPass()) {
                    board = board.applyPass();
                    if (board.canPlace(pos)) {
                        board = board.applyMove(pos);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            series.push(board.blackCount - board.whiteCount);
        }
        return series;
    } catch {
        return [0];
    }
}

function notationToPositionSafe(notation) {
    if (typeof notation !== 'string' || notation.length !== 2) return null;
    const letters = 'abcdefgh';
    const numbers = '12345678';
    const col = letters.indexOf(notation[0].toLowerCase());
    const row = numbers.indexOf(notation[1]);
    if (col === -1 || row === -1) return null;
    return row * 8 + col;
}

/**
 * @param {number[]} series
 * @returns {string} SVG markup
 */
function renderSparklineSvg(series, options = {}) {
    const w = 320;
    const h = 100;
    const pad = 8;
    const n = Math.max(1, series.length);
    const cursor = Number.isInteger(options.cursor) ? options.cursor : null;
    const hover = Number.isInteger(options.hover) ? options.hover : null;
    let min = Infinity;
    let max = -Infinity;
    for (const v of series) {
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
    const zeroY = toY(0);

    const grid = [max, 0, min]
        .map((v) => {
            const y = toY(v);
            const label = v === 0 ? '0' : String(v);
            return `
              <line x1="${pad}" y1="${y.toFixed(1)}" x2="${(w - pad).toFixed(1)}" y2="${y.toFixed(1)}" stroke="var(--line)" stroke-width="1"/>
              <text x="${pad.toFixed(1)}" y="${(y - 4).toFixed(1)}" fill="var(--muted)" font-size="10">${escapeHtml(label)}</text>
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
    return `
      <svg viewBox="0 0 ${w} ${h}" width="100%" height="100%" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="graph">
        ${grid}
        <line x1="${pad}" y1="${zeroY.toFixed(1)}" x2="${w - pad}" y2="${zeroY.toFixed(1)}" stroke="rgba(15,23,42,0.18)" stroke-width="1"/>
        ${hoverLine}
        ${cursorLine}
        <polyline points="${points}" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
        ${dots}
      </svg>
    `.trim();
}
