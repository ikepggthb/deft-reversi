/**
 * @fileoverview GameService - ゲーム進行を統括するアプリケーションサービス
 */

import { Game } from '../domain/game.js';
import { turnToLower, Turn, positionToNotation } from '../domain/types.js';
import { AiEngine } from '../infrastructure/ai-engine.js';
import { HintService } from './hint-service.js';
import { SettingsService } from './settings-service.js';
import { OpeningService } from './opening-service.js';
import { SessionStateManager } from './session-state-manager.js';
import { PassHandler } from './pass-handler.js';
import { UI } from '../ui/ui.js';
import { notationToPosition } from '../domain/types.js';
import { Board } from '../domain/board.js';

function rotatePos90Clockwise(pos) {
    const x = pos % 8;
    const y = Math.floor(pos / 8);
    const nx = 7 - y;
    const ny = x;
    return ny * 8 + nx;
}

function rotateBitsClockwise(bits, quarterTurns) {
    let turns = Number(quarterTurns) % 4;
    if (turns < 0) turns += 4;
    if (turns === 0) return bits;

    let current = bits;
    for (let t = 0; t < turns; t++) {
        let rotated = 0n;
        for (let pos = 0; pos < 64; pos++) {
            if (((current >> BigInt(pos)) & 1n) === 0n) continue;
            const np = rotatePos90Clockwise(pos);
            rotated |= 1n << BigInt(np);
        }
        current = rotated;
    }
    return current;
}

/**
 * UI層へ渡すビューモデル
 * @typedef {Object} GameViewModel
 * @property {bigint} blackBits 黒石のビットボード
 * @property {bigint} whiteBits 白石のビットボード
 * @property {bigint} legalMovesBits 合法手のビットボード
 * @property {string} nextTurn 次の手番
 * @property {number[] | null} eval 評価値
 * @property {number | null} lastMove 最後の着手位置
 * @property {string | null} currentHumanOpening 現在の定石名
 * @property {number | null} humanOpeningNextPosition 定石の次の手
 * @property {boolean} pendingAutoStart AI自動開始待ちかどうか
 */

/** @typedef {import('./session-state-manager.js').HistorySnapshot} HistorySnapshot */
/** @typedef {import('./session-state-manager.js').SessionState} SessionState */

/**
 * ゲーム進行を統括するアプリケーションサービス
 */
export class GameService {
    /**
 * @param {Object} [dependencies] オプショナルな依存関係
 * @param {Object} [dependencies.ui]
 * @param {AiEngine} [dependencies.aiEngine]
 * @param {HintService} [dependencies.hintService]
 * @param {SettingsService} [dependencies.settingsService]
 * @param {OpeningService} [dependencies.openingService]
 * @param {boolean} [dependencies.loadOpeningData]
 * @param {boolean} [dependencies.autoStart] 起動時にゲームを開始するか（デフォルト: false）
 */
    constructor(dependencies = {}) {
        // 設定サービスを先に初期化
        /** @private */
        this._settingsService = dependencies.settingsService ?? new SettingsService();

        // インフラ層
        /** @private */
        this._aiEngine = dependencies.aiEngine ?? new AiEngine();
        /** @private */
        this._hintService = dependencies.hintService ?? new HintService(this._aiEngine);
        /** @private */
        this._openingService = dependencies.openingService ?? new OpeningService();

        // 定石データをロード
        if (dependencies.loadOpeningData !== false) {
            this._loadOpeningData();
        }

        // セッション状態管理
        /** @private */
        this._stateManager = new SessionStateManager();

        // パス処理
        /** @private */
        this._passHandler = new PassHandler({
            onPassAnimation: () => this._handlePassAnimation(),
            onSnapshot: (state) => this._snapshotState(state),
            onRender: () => this._render(),
        });

        // AI制御
        /** @private */
        this._isAiThinking = false;
        /** @private */
        this._aiToken = 0;

        // 排他制御用キュー
        /** @private */
        this._queue = Promise.resolve();

        // プレイヤー情報
        /** @private */
        this._blackPlayerName = undefined;
        /** @private */
        this._whitePlayerName = undefined;

        // UIを最後に初期化（thisを渡す）
        /** @private */
        this._ui =
            dependencies.ui ??
            new UI(this, {
                aiEnabled: this._settingsService.enableAi,
                aiLevel: this._settingsService.aiLevel,
                analysisLevel: this._settingsService.analysisLevel,
                analysisRunLevel: this._settingsService.analysisRunLevel,
                blackAiLevel: this._settingsService.blackAiLevel,
                whiteAiLevel: this._settingsService.whiteAiLevel,
                aiTurn: this._settingsService.aiTurn,
                humanOpening: this._settingsService.humanOpening,
            });

        this._initializeEngine();
        this._ui.render(undefined, this._blackPlayerName, this._whitePlayerName);

        if (dependencies.autoStart === true) {
            this._runExclusive(async () => {
                await this._startNewGame();
            });
        }
    }

    /** @private @returns {SessionState} */
    _activeState() {
        return this._stateManager.getActiveState();
    }

    /** @private @returns {SessionState} */
    get _mainState() {
        return this._stateManager.getMainState();
    }

    /** @private @returns {SessionState | null} */
    get _studyState() {
        return this._stateManager.getStudyState();
    }

    /** @private @returns {boolean} */
    get _studyModeEnabled() {
        return this._stateManager.isStudyModeEnabled;
    }

    /** @private @returns {import('../domain/game.js').Game | null} */
    get _game() {
        return this._stateManager.game;
    }

    /** @private @param {import('../domain/game.js').Game | null} value */
    set _game(value) {
        this._stateManager.game = value;
    }

    /** @private @returns {HistorySnapshot[]} */
    get _history() {
        return this._stateManager.history;
    }

    /** @private @param {HistorySnapshot[]} value */
    set _history(value) {
        this._stateManager.history = value;
    }

    /** @private @returns {HistorySnapshot[]} */
    get _future() {
        return this._stateManager.future;
    }

    /** @private @param {HistorySnapshot[]} value */
    set _future(value) {
        this._stateManager.future = value;
    }

    /** @private @returns {(number | null)[] | null} */
    get _hintScores() {
        return this._stateManager.hintScores;
    }

    /** @private @param {(number | null)[] | null} value */
    set _hintScores(value) {
        this._stateManager.hintScores = value;
    }

    /** @private @returns {boolean} */
    get _pendingAutoStart() {
        return this._stateManager.pendingAutoStart;
    }

    /** @private @param {boolean} value */
    set _pendingAutoStart(value) {
        this._stateManager.pendingAutoStart = value;
    }

    /** @private @returns {ReadonlyArray<string>} */
    get _studyBranchStartRecord() {
        return this._stateManager.studyBranchStartRecord;
    }

    // === 公開API（UIから呼ばれる） ===

    /**
     * 盤面クリック処理
     * @param {number} position
     */
    handleBoardClick(position) {
        this._runExclusive(() => this._handleHumanMove(position));
    }

    /**
     * 新規ゲーム開始
     */
    startNewGame() {
        this._runExclusive(async () => {
            this._clearStudyState();
            this._aiToken += 1;
            this._isAiThinking = false;
            this._hintService.cancel();
            this._aiEngine.terminate();
            this._initializeEngine();
            await this._startNewGame();
        });
    }

    /**
     * セッションを開始（設定 + 初期局面）
     * - 対戦（AI自動着手）/Free/Analysis の共通入口として使用する
     * @param {Object} options
     * @param {boolean} options.aiEnabled
     * @param {number} options.aiLevel
     * @param {'black' | 'white' | 'both'} options.aiTurn
     * @param {number | null} options.humanOpening
     * @param {string} [options.blackPlayerName]
     * @param {string} [options.whitePlayerName]
     * @param {string[] | null} [options.record] e.g. ["f5","d6","pass",...]
     * @param {boolean} [options.enableHint] ヒントを有効化する（デフォルト: false）
     * @param {number} [options.hintLevel] ヒント計算の深さ（enableHint=trueの場合のみ反映）
     * @param {boolean} [options.truncateHistoryToCurrent] 現局面をUndo/Redoの基点にする（Analysis/Edit用）
     * @param {boolean} [options.resetEngine] Worker/WASMを再初期化する（デフォルト: false）
     * @param {boolean} [options.runAiOnStart] セッション開始直後のAI自動着手を許可する（デフォルト: true）
     * @param {number} [options.analysisLevel]
     * @param {number} [options.blackAiLevel]
     * @param {number} [options.whiteAiLevel]
     * @param {boolean} [options.pendingAutoStart]
     */
    startSession(options) {
        this._runExclusive(async () => {
            this._clearStudyState();
            const {
                aiEnabled,
                aiLevel,
                analysisLevel,
                blackAiLevel,
                whiteAiLevel,
                aiTurn,
                humanOpening,
                blackPlayerName,
                whitePlayerName,
                record,
                enableHint = false,
                hintLevel,
                truncateHistoryToCurrent = false,
                resetEngine = false,
                runAiOnStart = true,
                pendingAutoStart = false,
            } = options;

            this._settingsService.enableAi = Boolean(aiEnabled);
            this._settingsService.aiLevel = aiLevel;
            if (Number.isInteger(analysisLevel)) this._settingsService.analysisLevel = analysisLevel;
            if (Number.isInteger(blackAiLevel)) this._settingsService.blackAiLevel = blackAiLevel;
            if (Number.isInteger(whiteAiLevel)) this._settingsService.whiteAiLevel = whiteAiLevel;
            this._settingsService.aiTurn = aiTurn;
            this._settingsService.humanOpening = humanOpening ?? null;
            this._settingsService.save();

            if (typeof blackPlayerName === 'string') this._blackPlayerName = blackPlayerName;
            if (typeof whitePlayerName === 'string') this._whitePlayerName = whitePlayerName;

            this._aiToken += 1;
            this._isAiThinking = false;
            this._pendingAutoStart = Boolean(pendingAutoStart);
            this._hintService.cancel();

            this._settingsService.enableHint = Boolean(enableHint);
            if (Number.isInteger(hintLevel)) {
                this._settingsService.hintLevel = hintLevel;
            }

            if (resetEngine) {
                this._aiEngine.terminate();
                this._initializeEngine();
            }

            if (record && Array.isArray(record) && record.length > 0) {
                this._loadFromRecord(record);
            } else {
                this._createNewGameState();
            }

            if (truncateHistoryToCurrent) {
                this._history = [];
                this._future = [];
            }

            this._render();
            if (runAiOnStart && !this._pendingAutoStart && this._aiEngine.isReady && this._settingsService.enableAi) {
                await this._maybeRunAiTurn();
            }
            this._refreshHint();
        });
    }

    /**
     * 現在局面を回転する（盤面のみ変換し、棋譜はクリアする）
     * @param {{ rotateQuarterTurns: 1 | 2 | 3 }} options
     */
    transformBoard(options) {
        this._runExclusive(async () => {
            if (!this._game) return;
            this._clearStudyState();

            const turnsRaw = Number(options?.rotateQuarterTurns ?? 1);
            const turns = Math.max(1, Math.min(3, Math.trunc(turnsRaw)));

            const board = this._game.board;
            const blackBits = rotateBitsClockwise(board.blackBits, turns);
            const whiteBits = rotateBitsClockwise(board.whiteBits, turns);
            const rotatedBoard = Board.fromBits(blackBits, whiteBits, board.nextTurn);

            this._pushHistory();
            this._clearFuture();
            this._hintService.cancel();
            this._hintScores = null;
            this._game = new Game(rotatedBoard, [], null);
            this._render();

            if (this._settingsService.enableAi) {
                await this._maybeRunAiTurn();
            }
            this._refreshHint();
        });
    }

    /**
     * 棋譜（文字列）から局面を作る（Free/Analysis用）
     * @param {string} moveListText e.g. "f5d6c3" or "f5 d6 c3" or includes "pass"
     * @param {{ autoPass?: boolean }} [options]
     * @returns {{ ok: true, record: string[] } | { ok: false, error: string }}
     */
    parseMoveList(moveListText, options = {}) {
        const autoPass = options.autoPass ?? true;
        if (typeof moveListText !== 'string') {
            return { ok: false, error: 'Invalid input' };
        }
        const raw = moveListText.trim();
        if (!raw) return { ok: true, record: [] };

        // tokenize: whitespace/comma separated OR compact pairs (e.g. f5d6c3)
        /** @type {string[]} */
        let tokens = [];
        if (/[,\s]/.test(raw)) {
            tokens = raw
                .split(/[\s,]+/g)
                .map((t) => t.trim())
                .filter(Boolean);
        } else {
            tokens = [];
            let i = 0;
            while (i < raw.length) {
                const rest = raw.slice(i).toLowerCase();
                if (rest.startsWith('pass')) {
                    tokens.push('pass');
                    i += 4;
                    continue;
                }
                if (i + 2 > raw.length) {
                    return { ok: false, error: 'Invalid move list (odd length)' };
                }
                tokens.push(raw.slice(i, i + 2));
                i += 2;
            }
        }

        /** @type {string[]} */
        const record = [];
        for (const token of tokens) {
            const t = token.toLowerCase();
            if (t === 'pass') {
                record.push('pass');
                continue;
            }
            const pos = notationToPosition(t);
            if (pos === null) {
                return { ok: false, error: `Invalid move: ${token}` };
            }
            record.push(t);
        }

        if (!autoPass) return { ok: true, record };

        // auto-pass normalization is handled in _loadFromRecord (board legality required)
        return { ok: true, record };
    }

    /**
     * Undo操作
     */
    undo() {
        this._runExclusive(() => this._undo());
    }

    /**
     * Redo操作
     */
    redo() {
        this._runExclusive(() => this._redo());
    }

    /** @returns {boolean} */
    get canUndo() {
        return this._history.length > 0;
    }

    /** @returns {boolean} */
    get canRedo() {
        return this._future.length > 0;
    }

    /**
     * Redoできるだけ進める（Analysis/Edit 用）
     */
    redoToEnd() {
        this._runExclusive(() => {
            while (this._future.length > 0) {
                this._redo();
            }
        });
    }

    /**
     * Undoできるだけ戻る（Free用）
     */
    undoToStart() {
        this._runExclusive(() => {
            while (this._history.length > 0) {
                this._undo();
            }
        });
    }

    /**
     * 指定手数の局面へ移動
     * @param {number} ply
     */
    goToPly(ply) {
        this._runExclusive(() => {
            if (!this._game) return;
            const target = Math.max(0, Math.trunc(Number(ply) || 0));
            while (this._game && this._game.record.length > target && this._history.length > 0) {
                this._undo();
            }
            while (this._game && this._game.record.length < target && this._future.length > 0) {
                this._redo();
            }
        });
    }

    /**
     * 現在の棋譜（読み取り専用）
     * @returns {ReadonlyArray<string>}
     */
    get record() {
        return this._game?.record ?? [];
    }

    /**
     * 本線の棋譜
     * @returns {ReadonlyArray<string>}
     */
    get mainRecord() {
        return this._mainState.game?.record ?? [];
    }

    /**
     * 検討開始時点の本線棋譜
     * @returns {ReadonlyArray<string>}
     */
    get studyBranchStartRecord() {
        return this._stateManager.studyBranchStartRecord;
    }

    /** @returns {boolean} */
    get hintEnabled() {
        return this._settingsService.enableHint;
    }

    /** @returns {number} */
    get hintLevel() {
        return this._settingsService.hintLevel;
    }

    /** @returns {number} */
    get analysisLevel() {
        return this._settingsService.analysisLevel;
    }

    get analysisRunLevel() {
        return this._settingsService.analysisRunLevel;
    }

    /** @returns {boolean} */
    get isAutoStartPending() {
        return this._pendingAutoStart;
    }

    /** @returns {boolean} */
    get isStudyModeEnabled() {
        return this._studyModeEnabled;
    }

    startAutoPlay() {
        this._runExclusive(async () => {
            if (!this._mainState.pendingAutoStart) return;
            this._mainState.pendingAutoStart = false;
            this._render();
            await this._maybeRunAiTurn(this._mainState);
        });
    }

    /**
     * 棋譜の各手順の「最善手/評価」を解析する（評価は黒視点に正規化）
     * - ply=0 は初期局面（次の手番の最善手/評価）
     * - ply=i は i手進んだ局面（次の手番の最善手/評価）
     * @param {string[]} record
     * @param {number} level
     * @param {{ onProgress?: (p: { completed: number, total: number, currentPly: number, ply: number }) => void }} [options]
     * @returns {Promise<{ ok: true, analysis: Array<{ ply: number, nextTurn: import('../domain/types.js').Turn, evalToMove: number, evalBlack: number, bestMove: string | null }>, error?: undefined } | { ok: false, analysis?: undefined, error: string }>}
     */
    async analyzeRecord(record, level, options = {}) {
        try {
            if (!this._aiEngine.isReady) {
                return { ok: false, error: 'Engine not ready' };
            }
            const safeRecord = Array.isArray(record) ? record : [];
            const total = safeRecord.length;
            const depth = Math.max(1, Math.min(24, Math.trunc(Number(level) || 8)));
            const totalPositions = total + 1;

            /** @type {Array<{ ply: number, nextTurn: import('../domain/types.js').Turn, evalToMove: number, evalBlack: number, bestMove: string | null }>} */
            const analysis = new Array(totalPositions);
            /** @type {Board[]} */
            const boards = [Board.initial()];

            let board = Board.initial();

            for (let ply = 0; ply < total; ply++) {
                const mRaw = safeRecord[ply];
                const m = String(mRaw).toLowerCase();
                if (m === 'pass') {
                    if (!board.mustPass()) {
                        return { ok: false, error: `Invalid pass at ply ${ply + 1}` };
                    }
                    board = board.applyPass();
                    boards.push(board);
                    continue;
                }

                const pos = notationToPosition(m);
                if (pos === null) {
                    return { ok: false, error: `Invalid move: ${mRaw}` };
                }

                if (!board.canPlace(pos)) {
                    if (board.mustPass()) {
                        board = board.applyPass();
                    }
                }

                if (!board.canPlace(pos)) {
                    return { ok: false, error: `Illegal move: ${mRaw}` };
                }
                board = board.applyMove(pos);
                boards.push(board);
            }

            let completed = 0;
            for (let ply = total; ply >= 0; ply--) {
                const analysisPoint = await this._analyzeBoardState(boards[ply], depth);
                if (!analysisPoint.ok) {
                    return { ok: false, error: analysisPoint.error };
                }

                analysis[ply] = {
                    ply,
                    nextTurn: boards[ply].nextTurn,
                    evalToMove: analysisPoint.evalToMove,
                    evalBlack: analysisPoint.evalBlack,
                    bestMove: analysisPoint.bestMoveText,
                };
                completed += 1;
                options.onProgress?.({ completed, total: totalPositions, currentPly: ply, ply });
            }

            return { ok: true, analysis };
        } catch (e) {
            return { ok: false, error: e?.message ?? String(e) };
        }
    }

    /**
     * 現在局面の評価値を黒視点で返す
     * @param {number} level
     * @returns {Promise<{ ok: true, evalBlack: number } | { ok: false, error: string }>}
     */
    async evaluateCurrentPosition(level) {
        return this.evaluatePosition(level, { scope: 'active' });
    }

    /**
     * 指定スコープの局面を評価する
     * @param {number} level
     * @param {{ scope?: 'active' | 'main' | 'study' }} [options]
     * @returns {Promise<{ ok: true, evalBlack: number } | { ok: false, error: string }>}
     */
    async evaluatePosition(level, options = {}) {
        try {
            if (!this._aiEngine.isReady) {
                return { ok: false, error: 'Engine not ready' };
            }
            const scope = options.scope ?? 'active';
            const state =
                scope === 'main'
                    ? this._mainState
                    : scope === 'study'
                      ? this._studyState
                      : this._activeState();
            if (!state?.game) {
                return { ok: false, error: 'Game not started' };
            }

            const depth = Math.max(1, Math.min(24, Math.trunc(Number(level) || 8)));
            const analysisPoint = await this._analyzeBoardState(state.game.board, depth);
            if (!analysisPoint.ok) {
                return { ok: false, error: analysisPoint.error };
            }

            return {
                ok: true,
                evalBlack: analysisPoint.evalBlack,
            };
        } catch (e) {
            return { ok: false, error: e?.message ?? String(e) };
        }
    }

    /**
     * 現在のゲーム結果（ゲーム終了時のみ意味がある）
     * @returns {{ blackCount: number, whiteCount: number, winner: import('../domain/types.js').Turn | null } | null}
     */
    get result() {
        if (!this._game) return null;
        if (!this._game.isGameOver) return null;
        return this._game.getResult();
    }

    /**
     * AIの最善手を1手だけ打つ（Free/Analysis用）
     * @param {number} level
     */
    playAiMoveOnce(level) {
        this._runExclusive(async () => {
            if (!this._game) return;
            if (!this._aiEngine.isReady) {
                this._ui.logError('AIエンジンの準備ができていません。');
                return;
            }
            if (this._isAiThinking) return;
            if (this._game.isGameOver) {
                await this._showGameOver();
                return;
            }

            const passed = await this._handleForcedPasses();
            if (this._game.isGameOver) {
                await this._showGameOver();
                return;
            }
            if (passed) {
                this._refreshHint();
                return;
            }

            this._isAiThinking = true;
            const token = ++this._aiToken;
            try {
                const board = this._game.board;
                const { bestMove } = await this._aiEngine.solveTurn(
                    board.playerBits,
                    board.opponentBits,
                    level
                );

                if (token !== this._aiToken) return;

                this._pushHistory();
                this._clearFuture();
                if (bestMove === null || bestMove < 0) {
                    await this._handlePassAnimation();
                    this._game = this._game.applyPass();
                } else {
                    this._game = this._game.applyMove(bestMove);
                }
                this._render();

                if (this._game.isGameOver) {
                    await this._showGameOver();
                    return;
                }
                const { game: updatedGame } = await this._passHandler.handlePostMovePassSimple(this._game);
                this._game = updatedGame;
                this._refreshHint();
            } catch (error) {
                if (token !== this._aiToken) return;
                console.error('playAiMoveOnce failed', error);
                this._ui.logError('AIの思考に失敗しました。');
            } finally {
                this._isAiThinking = false;
            }
        });
    }

    /**
     * ヒント表示切り替え
     */
    toggleHint() {
        this._runExclusive(() => this._toggleHint());
    }

    /**
     * 深いヒント計算
     * @param {number} depth
     */
    requestDeepHint(depth) {
        this._runExclusive(() => this._deepHint(depth));
    }

    /**
     * ヒント計算深さを更新（表示のON/OFFは変えない）
     * @param {number} level
     */
    setHintLevel(level) {
        this._runExclusive(() => {
            const n = Number(level);
            if (!Number.isFinite(n)) return;
            const depth = Math.max(1, Math.min(24, Math.trunc(n)));
            this._settingsService.hintLevel = depth;
            this._hintService.cancel();
            if (this._settingsService.enableHint) {
                this._refreshHint();
            }
        });
    }

    /**
     * AI設定を更新
     * @param {boolean} enabled
     * @param {number} level
     * @param {'black' | 'white' | 'both'} turn
     */
    updateAISettings(enabled, level, turn, blackLevel = level, whiteLevel = level) {
        this._settingsService.enableAi = enabled;
        this._settingsService.aiLevel = level;
        this._settingsService.blackAiLevel = blackLevel;
        this._settingsService.whiteAiLevel = whiteLevel;
        this._settingsService.aiTurn = turn;
        this._settingsService.save();
        this._render();
    }

    /**
     * 評価レベルを更新
     * @param {number} level
     */
    setAnalysisLevel(level) {
        const n = Number(level);
        if (!Number.isFinite(n)) return;
        const depth = Math.max(1, Math.min(24, Math.trunc(n)));
        this._settingsService.analysisLevel = depth;
        this._settingsService.hintLevel = depth;
        this._settingsService.save();
        this._hintService.cancel();
        this._refreshHint();
    }

    /**
     * 分析レベルを更新
     * @param {number} level
     */
    setAnalysisRunLevel(level) {
        const n = Number(level);
        if (!Number.isFinite(n)) return;
        const depth = Math.max(1, Math.min(24, Math.trunc(n)));
        this._settingsService.analysisRunLevel = depth;
        this._settingsService.save();
    }

    /**
     * 盤面評価表示の有効/無効を切り替える
     * @param {boolean} enabled
     */
    setBoardEvalEnabled(enabled) {
        this._runExclusive(() => {
            this._settingsService.enableHint = Boolean(enabled);
            this._hintService.cancel();
            if (this._settingsService.enableHint) {
                this._refreshHint();
            } else {
                this._hintScores = null;
                this._render();
            }
        });
    }

    /**
     * 検討モードを切り替える
     * @param {boolean} enabled
     */
    setStudyMode(enabled) {
        this._runExclusive(() => {
            const next = Boolean(enabled);
            if (next === this._studyModeEnabled) return;

            if (next) {
                const mainRecord = this._mainState.game?.record ?? [];
                this._stateManager.enableStudyMode([...mainRecord]);
            } else {
                this._stateManager.disableStudyMode();
            }

            this._render();
            this._refreshHint();
        });
    }

    /**
     * プレイヤー名を設定
     * @param {string} black
     * @param {string} white
     */
    setPlayerNames(black, white) {
        this._blackPlayerName = black;
        this._whitePlayerName = white;
        this._render();
    }

    /**
     * 定石設定
     * @param {number | null} opening
     */
    setHumanOpening(opening) {
        this._settingsService.humanOpening = opening;
        this._settingsService.save();
        this._render();
        this._refreshHint();
    }

    // === プライベートメソッド: 初期化 ===

    /**
     * 定石データをロード
     * @private
     */
    async _loadOpeningData() {
        try {
            const response = await fetch('./assets/opening.txt');
            if (response.ok) {
                const text = await response.text();
                this._openingService.load(text);
            }
        } catch (error) {
            console.warn('Failed to load opening data:', error);
        }
    }

    /** @private */
    _initializeEngine() {
        this._aiEngine.initialize(
            () => {
                this._ui.onAIReady();
                this._runExclusive(async () => {
                    if (this._mainState.game && this._settingsService.enableAi && !this._mainState.pendingAutoStart) {
                        await this._maybeRunAiTurn(this._mainState);
                    }
                    this._refreshHint();
                });
            },
            (error) => {
                this._ui.logError(
                    `AIエンジンの初期化に失敗しました。ページを再読み込みするか、Worker / WASM / 評価データが正しく配信されているか確認してください: ${error.message}`
                );
            }
        );
    }

    /**
     * 盤面状態を評価し、黒視点評価へ正規化する
     * @private
     * @param {Board} board
     * @param {number} depth
     * @returns {Promise<{ ok: true, evalToMove: number, evalBlack: number, bestMoveText: string | null } | { ok: false, error: string }>}
     */
    async _analyzeBoardState(board, depth) {
        try {
            if (board.isGameOver()) {
                const evalBlack = board.blackCount - board.whiteCount;
                return {
                    ok: true,
                    evalToMove: board.nextTurn === Turn.BLACK ? evalBlack : -evalBlack,
                    evalBlack,
                    bestMoveText: null,
                };
            }

            if (board.mustPass()) {
                const passedBoard = board.applyPass();
                const nested = await this._analyzeBoardState(passedBoard, depth);
                if (!nested.ok) return nested;
                return {
                    ok: true,
                    evalToMove: board.nextTurn === Turn.BLACK ? nested.evalBlack : -nested.evalBlack,
                    evalBlack: nested.evalBlack,
                    bestMoveText: 'pass',
                };
            }

            const { bestMove, eval: evalToMove } = await this._aiEngine.solveTurn(
                board.playerBits,
                board.opponentBits,
                depth
            );
            const evalBlack = board.nextTurn === Turn.BLACK ? evalToMove : -evalToMove;
            return {
                ok: true,
                evalToMove,
                evalBlack,
                bestMoveText: bestMove === null ? 'pass' : positionToNotation(bestMove),
            };
        } catch (error) {
            return { ok: false, error: error?.message ?? String(error) };
        }
    }

    // === プライベートメソッド: 排他制御 ===

    /**
     * 排他制御付きで実行
     * @private
     * @param {() => Promise<void>} fn
     * @returns {Promise<void>}
     */
    _runExclusive(fn) {
        this._queue = this._queue
            .then(fn)
            .catch((error) => {
                console.error('Game error:', error);
                this._ui.logError(`エラーが発生しました: ${error?.message ?? String(error)}`);
            });
        return this._queue;
    }

    // === プライベートメソッド: ゲーム進行 ===

    /**
     * 新規ゲームを開始
     * @private
     */
    async _startNewGame() {
        this._createNewGameState();
        this._pendingAutoStart = this._shouldPendAutoStart();
        this._render();

        if (!this._pendingAutoStart && this._isAiTurn()) {
            await this._maybeRunAiTurn();
        }
        this._refreshHint();
    }

    /**
     * 新規ゲーム状態を作る（副作用なし）
     * @private
     */
    _createNewGameState() {
        this._history = [];
        this._future = [];
        this._hintScores = null;
        this._game = Game.newGame();
    }

    /**
     * 棋譜からゲーム状態を構築（Undo可能な履歴も生成）
     * @private
     * @param {string[]} record
     */
    _loadFromRecord(record) {
        this._history = [];
        this._future = [];
        this._hintScores = null;
        let game = Game.newGame();

        const pushSnapshot = () => {
            this._game = game;
            this._history.push(this._snapshot());
        };

        for (const move of record) {
            const t = String(move).toLowerCase();
            if (!t) continue;
            pushSnapshot();

            if (t === 'pass') {
                game = game.applyPass();
                continue;
            }

            const pos = notationToPosition(t);
            if (pos === null) {
                throw new Error(`Invalid move: ${move}`);
            }
            if (!game.canPlace(pos)) {
                // 記録がパス省略形式の場合、強制パスを自動挿入
                if (game.mustPass) {
                    game = game.applyPass();
                    pushSnapshot();
                }
                if (!game.canPlace(pos)) {
                    throw new Error(`Illegal move: ${move}`);
                }
            }
            game = game.applyMove(pos);

            // 次手番が強制パスで、入力が省略されていそうなら自動で入れる
            if (game.mustPass) {
                game = game.applyPass();
            }
        }

        this._game = game;
    }

    /**
     * 人間の着手を処理
     * @private
     * @param {number} position
     */
    async _handleHumanMove(position) {
        const activeState = this._activeState();

        // 前提条件チェック
        const checkResult = await this._checkHumanMovePrerequisites(activeState, position);
        if (checkResult !== 'proceed') return;

        // 着手を適用
        try {
            const applyResult = await this._applyHumanMove(position);
            if (applyResult === 'game-over') return;

            await this._finishHumanMove();
        } catch (error) {
            console.error('Human move failed', error);
            this._ui.logError('その手は打てません。');
        }
    }

    /**
     * 人間の着手の前提条件をチェック
     * @private
     * @param {SessionState} activeState
     * @param {number} position
     * @returns {Promise<'proceed' | 'blocked'>}
     */
    async _checkHumanMovePrerequisites(activeState, position) {
        if (this._game.isGameOver) {
            await this._showGameOver(activeState);
            return 'blocked';
        }
        if (!this._studyModeEnabled && activeState.pendingAutoStart && this._isAiTurn(activeState)) {
            this._ui.logInfo('対局開始を押すと AI が着手します。');
            return 'blocked';
        }
        if (!this._studyModeEnabled && this._isAiTurn(activeState)) {
            this._ui.logInfo('AIの手番です。お待ちください。');
            return 'blocked';
        }

        // 強制パス処理
        const passed = await this._handleForcedPasses(activeState);
        if (this._game.isGameOver) {
            await this._showGameOver();
            return 'blocked';
        }
        if (passed) {
            if (!this._studyModeEnabled) {
                await this._maybeRunAiTurn(this._mainState);
            }
            return 'blocked';
        }

        // 着手可能性チェック
        if (!this._game.canPlace(position)) {
            this._ui.logError('その手は打てません。');
            return 'blocked';
        }

        return 'proceed';
    }

    /**
     * 人間の着手を適用
     * @private
     * @param {number} position
     * @returns {Promise<'continue' | 'game-over'>}
     */
    async _applyHumanMove(position) {
        this._pushHistory();
        this._clearFuture();
        this._game = this._game.applyMove(position);
        this._render();

        if (this._game.isGameOver) {
            await this._showGameOver();
            return 'game-over';
        }

        // 相手がパスすべき場合
        const { game: updatedGame } = await this._passHandler.handlePostMovePassSimple(this._game);
        this._game = updatedGame;
        if (this._game.isGameOver) {
            await this._showGameOver();
            return 'game-over';
        }

        return 'continue';
    }

    /**
     * 人間の着手後の処理
     * @private
     */
    async _finishHumanMove() {
        this._refreshHint();
        if (!this._studyModeEnabled) {
            await this._maybeRunAiTurn(this._mainState);
        }
    }

    /**
     * AIの手番かどうか
     * @private
     * @returns {boolean}
     */
    _isAiTurn(state = this._activeState()) {
        if (!state?.game) return false;
        if (this._settingsService.aiTurn === 'both') {
            return this._settingsService.enableAi;
        }
        return (
            this._settingsService.enableAi &&
            turnToLower(state.game.nextTurn) === this._settingsService.aiTurn
        );
    }

    /**
     * AI手番を実行（有効な場合）
     * @private
     */
    async _maybeRunAiTurn(state = this._mainState) {
        if (!this._settingsService.enableAi) return;
        if (!state?.game) return;
        if (state.pendingAutoStart) return;
        if (!this._isAiTurn(state)) return;
        await this._runAiTurn(state);
    }

    /**
     * AIの手番を実行
     * @private
     */
    async _runAiTurn(state = this._mainState) {
        if (!this._canRunAiTurn(state)) return;
        if (state.game.isGameOver) {
            await this._showGameOver(state);
            return;
        }

        const passed = await this._handleForcedPasses(state);
        if (!state.game || state.game.isGameOver) {
            await this._showGameOver(state);
            return;
        }
        if (passed) {
            await this._maybeRunAiTurn(state);
            return;
        }
        if (!this._isAiTurn(state)) return;

        this._prepareForAiThinking(state);
        const token = ++this._aiToken;
        let shouldContinue = false;

        try {
            const bestMove = await this._computeAiMove(state, token);
            if (bestMove === undefined) return; // token mismatch

            const result = await this._applyAiMove(state, bestMove);
            if (result === 'game-over') return;
            shouldContinue = result === 'continue';

            this._finishAiTurn(state);
        } catch (error) {
            if (token !== this._aiToken) return;
            console.error('AI move failed', error);
            this._ui.logError('AIの思考に失敗しました。');
        } finally {
            this._isAiThinking = false;
        }

        if (token === this._aiToken && shouldContinue) {
            await this._maybeRunAiTurn(state);
        }
    }

    /**
     * AI手番実行の前提条件チェック
     * @private
     * @param {SessionState} state
     * @returns {boolean}
     */
    _canRunAiTurn(state) {
        if (!this._aiEngine.isReady) return false;
        if (this._isAiThinking) return false;
        if (!state?.game) return false;
        return true;
    }

    /**
     * AI思考開始の準備
     * @private
     * @param {SessionState} state
     */
    _prepareForAiThinking(state) {
        if (state === this._activeState()) {
            this._hintService.cancel();
            state.hintScores = null;
        }
        this._render();
        this._isAiThinking = true;
    }

    /**
     * AIの最善手を計算
     * @private
     * @param {SessionState} state
     * @param {number} token
     * @returns {Promise<number | null | undefined>} undefined if token mismatch
     */
    async _computeAiMove(state, token) {
        const openingMatch = this._getOpeningMatch(state.game);

        if (openingMatch && openingMatch.nextPosition !== null) {
            this._ui.logInfo(`AIが定石「${openingMatch.name}」の手を打ちます...`);
            return openingMatch.nextPosition;
        }

        const board = state.game.board;
        const { bestMove, eval: evalScore } = await this._aiEngine.solveTurn(
            board.playerBits,
            board.opponentBits,
            this._currentAiLevel(state)
        );

        if (token !== this._aiToken) return undefined;
        return bestMove;
    }

    /**
     * AIの手を適用
     * @private
     * @param {SessionState} state
     * @param {number | null} bestMove
     * @returns {Promise<'continue' | 'game-over'>}
     */
    async _applyAiMove(state, bestMove) {
        state.history.push(this._snapshotState(state));
        state.future = [];

        if (bestMove === null || bestMove < 0) {
            this._ui.logInfo('AIはパスします。');
            await this._handlePassAnimation();
            state.game = state.game.applyPass();
            this._render();
            return 'continue';
        }

        state.game = state.game.applyMove(bestMove);
        this._render();

        if (state.game.isGameOver) {
            await this._showGameOver(state);
            return 'game-over';
        }

        await this._passHandler.handlePostMovePass(state);
        if (state.game.isGameOver) {
            await this._showGameOver(state);
            return 'game-over';
        }

        return 'continue';
    }

    /**
     * AI手番の終了処理
     * @private
     * @param {SessionState} state
     */
    _finishAiTurn(state) {
        if (state === this._activeState()) {
            this._refreshHint();
        } else {
            this._render();
        }
    }

    /**
     * 強制パスを処理
     * @private
     * @returns {Promise<boolean>} パスが発生したかどうか
     */
    async _handleForcedPasses(state = this._activeState()) {
        return this._passHandler.handleForcedPasses(state);
    }

    /**
     * パスアニメーション
     * @private
     */
    async _handlePassAnimation() {
        this._ui.drawPassMessage();
        await new Promise((resolve) => setTimeout(resolve, 600));
    }

    /**
     * ゲーム終了モーダルを表示
     * @private
     */
    async _showGameOver(state = this._activeState()) {
        if (!state?.game) return;
        if (this._studyModeEnabled && state === this._studyState) return;
        const { blackCount, whiteCount } = state.game.getResult();
        this._ui.showEndGameModal(
            blackCount,
            whiteCount,
            this._blackPlayerName,
            this._whitePlayerName,
            state.game.record.join(' ')
        );
    }

    // === プライベートメソッド: Undo/Redo ===

    /**
     * Undo操作
     * @private
     */
    _undo() {
        if (this._history.length === 0) return;
        this._future.push(this._snapshot());
        const snapshot = this._history.pop();
        this._restore(snapshot);
        this._hintService.cancel();
        this._refreshHint();
    }

    /**
     * Redo操作
     * @private
     */
    _redo() {
        if (this._future.length === 0) return;
        this._history.push(this._snapshot());
        const snapshot = this._future.pop();
        this._restore(snapshot);
        this._hintService.cancel();
        this._refreshHint();
    }

    /**
     * 履歴に追加
     * @private
     */
    _pushHistory() {
        this._stateManager.pushHistory();
    }

    /**
     * 未来を消去
     * @private
     */
    _clearFuture() {
        this._stateManager.clearFuture();
    }

    /**
     * スナップショットを取得
     * @private
     * @returns {HistorySnapshot}
     */
    _snapshot() {
        return this._stateManager.snapshot();
    }

    /**
     * 指定セッションのスナップショットを取得
     * @private
     * @param {SessionState} state
     * @returns {HistorySnapshot}
     */
    _snapshotState(state) {
        return this._stateManager.snapshotState(state);
    }

    /**
     * スナップショットから復元
     * @private
     * @param {HistorySnapshot} snapshot
     */
    _restore(snapshot) {
        this._stateManager.restore(snapshot);
        this._recomputePendingAutoStart(this._activeState());
        this._render();
    }

    /**
     * 検討状態を破棄する
     * @private
     */
    _clearStudyState() {
        this._stateManager.clearStudyState();
    }

    // === プライベートメソッド: ヒント ===

    /**
     * ヒント表示の切り替え
     * @private
     */
    _toggleHint() {
        this._settingsService.toggleHint();
        this._hintService.cancel();
        if (this._settingsService.enableHint) {
            this._refreshHint();
        } else {
            this._hintScores = null;
            this._render();
        }
    }

    /**
     * 深いヒント計算
     * @private
     * @param {number} level
     */
    _deepHint(level) {
        this._settingsService.enableHint = true;
        this._settingsService.hintLevel = level;
        this._hintService.cancel();
        this._refreshHint();
    }

    /**
     * ヒント計算をスケジュール
     * @private
     */
    _refreshHint() {
        if (!this._settingsService.enableHint) return;
        if (!this._aiEngine.isReady) return;
        const state = this._activeState();
        if (!state.game) return;

        this._hintService.scheduleRefresh(
            state.game.board,
            this._settingsService.analysisLevel,
            this._studyModeEnabled ? false : this._isAiTurn(state),
            (scores) => this._onHintUpdate(scores)
        );
    }

    /**
     * ヒント更新コールバック
     * @private
     * @param {(number | null)[] | null} scores
     */
    _onHintUpdate(scores) {
        this._hintScores = scores;
        this._render();
    }

    // === プライベートメソッド: 描画 ===

    /**
     * UIを更新
     * @private
     */
    _render() {
        if (!this._game) {
            this._ui.render(undefined, this._blackPlayerName, this._whitePlayerName);
            return;
        }
        const viewModel = this._toViewModel();
        this._ui.render(viewModel, this._blackPlayerName, this._whitePlayerName);
    }

    /**
     * ドメイン状態からビューモデルを生成
     * @private
     * @returns {GameViewModel}
     */
    _toViewModel() {
        const state = this._activeState();
        const board = state.game.board;
        const openingMatch = this._getOpeningMatch(state.game);

        return {
            blackBits: board.blackBits,
            whiteBits: board.whiteBits,
            legalMovesBits: board.legalMovesBits,
            nextTurn: board.nextTurn,
            eval: state.hintScores ?? null,
            lastMove: state.game.lastMove,
            currentHumanOpening: openingMatch?.name ?? null,
            humanOpeningNextPosition: openingMatch?.nextPosition ?? null,
            pendingAutoStart: this._studyModeEnabled ? false : state.pendingAutoStart,
        };
    }

    /**
     * 現在手番の AI レベルを返す
     * @private
     * @returns {number}
     */
    _currentAiLevel(state = this._activeState()) {
        if (!state?.game) return this._settingsService.aiLevel;
        if (this._settingsService.aiTurn !== 'both') {
            return this._settingsService.aiLevel;
        }
        return state.game.board.nextTurn === Turn.BLACK
            ? this._settingsService.blackAiLevel
            : this._settingsService.whiteAiLevel;
    }

    /**
     * 明示開始待ちにすべきかどうか
     * @private
     * @returns {boolean}
     */
    _shouldPendAutoStart() {
        return this._settingsService.enableAi && (
            this._settingsService.aiTurn === 'black' ||
            this._settingsService.aiTurn === 'both'
        );
    }

    /**
     * 現在局面に対して明示開始待ちを組み直す
     * @private
     * @param {SessionState} state
     */
    _recomputePendingAutoStart(state) {
        if (!state) return;
        if (this._studyModeEnabled && state === this._studyState) {
            state.pendingAutoStart = false;
            return;
        }
        state.pendingAutoStart = this._settingsService.enableAi && this._isAiTurn(state);
    }

    /**
     * 現在の定石マッチングを取得
     * @private
     * @returns {import('./opening-service.js').OpeningMatch | null}
     */
    _getOpeningMatch(game = this._game) {
        if (!this._openingService.isLoaded) return null;
        if (!game) return null;

        const selectedOpening = this._settingsService.humanOpening;
        if (selectedOpening === null || selectedOpening === undefined) {
            return null;
        }

        const recordMoves = game.record;
        return this._openingService.match(selectedOpening, recordMoves);
    }
}
