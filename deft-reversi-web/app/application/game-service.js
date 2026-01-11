/**
 * @fileoverview GameService - ゲーム進行を統括するアプリケーションサービス
 */

import { Game } from '../domain/game.js';
import { turnToLower, Turn, positionToNotation } from '../domain/types.js';
import { AiEngine } from '../infrastructure/ai-engine.js';
import { HintService } from './hint-service.js';
import { SettingsService } from './settings-service.js';
import { OpeningService } from './opening-service.js';
import { UI } from '../ui/ui.js';
import { notationToPosition } from '../domain/types.js';
import { Board } from '../domain/board.js';

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
 */

/**
 * 履歴スナップショット
 * @typedef {Object} HistorySnapshot
 * @property {import('../domain/game.js').Game} game
 * @property {(number | null)[] | null} hintScores
 */

/**
 * ゲーム進行を統括するアプリケーションサービス
 */
export class GameService {
    /**
     * @param {Object} [dependencies] オプショナルな依存関係
     * @param {Object} [dependencies.ui]
     * @param {boolean} [dependencies.autoStart] 起動時にゲームを開始するか（デフォルト: false）
     */
    constructor(dependencies = {}) {
        // 設定サービスを先に初期化
        /** @private */
        this._settingsService = new SettingsService();

        // インフラ層
        /** @private */
        this._aiEngine = new AiEngine();
        /** @private */
        this._hintService = new HintService(this._aiEngine);
        /** @private */
        this._openingService = new OpeningService();

        // 定石データをロード
        this._loadOpeningData();

        // ゲーム状態
        /** @private @type {Game | null} */
        this._game = null;
        /** @private @type {HistorySnapshot[]} */
        this._history = [];
        /** @private @type {HistorySnapshot[]} */
        this._future = [];

        // ヒント表示用スコア
        /** @private @type {(number | null)[] | null} */
        this._hintScores = null;

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
     * @param {'black' | 'white'} options.aiTurn
     * @param {number | null} options.humanOpening
     * @param {string} [options.blackPlayerName]
     * @param {string} [options.whitePlayerName]
     * @param {string[] | null} [options.record] e.g. ["f5","d6","pass",...]
     * @param {boolean} [options.enableHint] ヒントを有効化する（デフォルト: false）
     * @param {number} [options.hintLevel] ヒント計算の深さ（enableHint=trueの場合のみ反映）
     * @param {boolean} [options.truncateHistoryToCurrent] 現局面をUndo/Redoの基点にする（Analysis/Edit用）
     * @param {boolean} [options.resetEngine] Worker/WASMを再初期化する（デフォルト: false）
     */
    startSession(options) {
        this._runExclusive(async () => {
            const {
                aiEnabled,
                aiLevel,
                aiTurn,
                humanOpening,
                blackPlayerName,
                whitePlayerName,
                record,
                enableHint = false,
                hintLevel,
                truncateHistoryToCurrent = false,
                resetEngine = false,
            } = options;

            this._settingsService.enableAi = Boolean(aiEnabled);
            this._settingsService.aiLevel = aiLevel;
            this._settingsService.aiTurn = aiTurn;
            this._settingsService.humanOpening = humanOpening ?? null;
            this._settingsService.save();

            if (typeof blackPlayerName === 'string') this._blackPlayerName = blackPlayerName;
            if (typeof whitePlayerName === 'string') this._whitePlayerName = whitePlayerName;

            this._aiToken += 1;
            this._isAiThinking = false;
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
            if (this._aiEngine.isReady && this._settingsService.enableAi) {
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
     * 現在の棋譜（読み取り専用）
     * @returns {ReadonlyArray<string>}
     */
    get record() {
        return this._game?.record ?? [];
    }

    /** @returns {boolean} */
    get hintEnabled() {
        return this._settingsService.enableHint;
    }

    /** @returns {number} */
    get hintLevel() {
        return this._settingsService.hintLevel;
    }

    /**
     * 棋譜の各手順の「最善手/評価」を解析する（評価は黒視点に正規化）
     * - ply=0 は初期局面（次の手番の最善手/評価）
     * - ply=i は i手進んだ局面（次の手番の最善手/評価）
     * @param {string[]} record
     * @param {number} level
     * @param {{ onProgress?: (p: { ply: number, total: number }) => void }} [options]
     * @returns {Promise<{ ok: true, analysis: Array<{ ply: number, nextTurn: import('../domain/types.js').Turn, evalToMove: number, evalBlack: number, bestMove: string | null }>, error?: undefined } | { ok: false, analysis?: undefined, error: string }>}
     */
    async analyzeRecord(record, level, options = {}) {
        try {
            if (!this._aiEngine.isReady) {
                return { ok: false, error: 'Engine not ready' };
            }
            const total = Array.isArray(record) ? record.length : 0;
            const depth = Math.max(1, Math.min(24, Math.trunc(Number(level) || 8)));

            /** @type {Array<{ ply: number, nextTurn: import('../domain/types.js').Turn, evalToMove: number, evalBlack: number, bestMove: string | null }>} */
            const analysis = [];

            let board = Board.initial();

            for (let ply = 0; ply <= total; ply++) {
                options.onProgress?.({ ply, total });

                const { bestMove, eval: evalToMove } = await this._aiEngine.solveTurn(
                    board.playerBits,
                    board.opponentBits,
                    depth
                );

                const evalBlack = board.nextTurn === Turn.BLACK ? evalToMove : -evalToMove;
                const bestMoveText = bestMove === null ? 'pass' : positionToNotation(bestMove);

                analysis.push({
                    ply,
                    nextTurn: board.nextTurn,
                    evalToMove,
                    evalBlack,
                    bestMove: bestMoveText,
                });

                if (ply === total) break;

                const mRaw = record[ply];
                const m = String(mRaw).toLowerCase();
                if (m === 'pass') {
                    if (!board.mustPass()) {
                        return { ok: false, error: `Invalid pass at ply ${ply + 1}` };
                    }
                    board = board.applyPass();
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
            }

            return { ok: true, analysis };
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
                if (this._game.mustPass) {
                    await this._handlePassAnimation();
                    this._game = this._game.applyPass();
                    this._render();
                }
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
     * @param {'black' | 'white'} turn
     */
    updateAISettings(enabled, level, turn) {
        this._settingsService.enableAi = enabled;
        this._settingsService.aiLevel = level;
        this._settingsService.aiTurn = turn;
        this._settingsService.save();
    }

    /**
     * プレイヤー名を設定
     * @param {string} black
     * @param {string} white
     */
    setPlayerNames(black, white) {
        this._blackPlayerName = black;
        this._whitePlayerName = white;
    }

    /**
     * 定石設定
     * @param {number | null} opening
     */
    setHumanOpening(opening) {
        this._settingsService.humanOpening = opening;
        this._settingsService.save();
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
                    if (this._game && this._settingsService.enableAi) {
                        await this._maybeRunAiTurn();
                    }
                    this._refreshHint();
                });
            },
            (error) => {
                this._ui.logError(`AIエンジンで問題が発生しました: ${error.message}`);
            }
        );
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
        this._render();

        if (this._isAiTurn()) {
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
        if (this._isAiTurn()) {
            this._ui.logInfo('AIの手番です。お待ちください。');
            return;
        }
        if (this._game.isGameOver) {
            await this._showGameOver();
            return;
        }

        // 強制パス処理
        const passed = await this._handleForcedPasses();
        if (this._game.isGameOver) {
            await this._showGameOver();
            return;
        }
        if (passed) {
            await this._maybeRunAiTurn();
            return;
        }

        // 着手可能性チェック
        if (!this._game.canPlace(position)) {
            this._ui.logError('その手は打てません。');
            return;
        }

        // 着手を適用
        try {
            this._pushHistory();
            this._clearFuture();
            this._game = this._game.applyMove(position);
            this._render();

            if (this._game.isGameOver) {
                await this._showGameOver();
                return;
            }

            // 相手がパスすべき場合
            if (this._game.mustPass) {
                await this._handlePassAnimation();
                this._game = this._game.applyPass();
                this._render();
                if (this._game.isGameOver) {
                    await this._showGameOver();
                    return;
                }
            }

            this._refreshHint();
            await this._maybeRunAiTurn();
        } catch (error) {
            console.error('Human move failed', error);
            this._ui.logError('その手は打てません。');
        }
    }

    /**
     * AIの手番かどうか
     * @private
     * @returns {boolean}
     */
    _isAiTurn() {
        if (!this._game) return false;
        return (
            this._settingsService.enableAi &&
            turnToLower(this._game.nextTurn) === this._settingsService.aiTurn
        );
    }

    /**
     * AI手番を実行（有効な場合）
     * @private
     */
    async _maybeRunAiTurn() {
        if (!this._settingsService.enableAi) return;
        await this._runAiTurn();
    }

    /**
     * AIの手番を実行
     * @private
     */
    async _runAiTurn() {
        if (!this._aiEngine.isReady) return;
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
            await this._maybeRunAiTurn();
            return;
        }
        if (!this._isAiTurn()) return;

        // ヒントをクリア
        this._hintService.cancel();
        this._hintScores = null;
        this._render();

        this._isAiThinking = true;
        const token = ++this._aiToken;
        let shouldContinue = false;

        try {
            // 定石チェック: 定石が選択されていて、次の手があればそれを打つ
            const openingMatch = this._getOpeningMatch();
            let bestMove = null;

            if (openingMatch && openingMatch.nextPosition !== null) {
                bestMove = openingMatch.nextPosition;
                this._ui.logInfo(`AIが定石「${openingMatch.name}」の手を打ちます...`);
                console.log('AI: opening move', { name: openingMatch.name, position: bestMove });
            } else {
                // 定石がない場合は通常のAI思考
                this._ui.logInfo('AIが思考中です...');
                const board = this._game.board;
                const { bestMove: aiMove, eval: evalScore } = await this._aiEngine.solveTurn(
                    board.playerBits,
                    board.opponentBits,
                    this._settingsService.aiLevel
                );

                if (token !== this._aiToken) return;

                bestMove = aiMove;
                console.log('AI solveTurn result:', { bestMove, eval: evalScore });
            }

            if (token !== this._aiToken) return;

            if (bestMove === null || bestMove < 0) {
                this._ui.logInfo('AIはパスします。');
                await this._handlePassAnimation();
                this._pushHistory();
                this._clearFuture();
                this._game = this._game.applyPass();
                this._render();
                shouldContinue = true;
            } else {
                this._pushHistory();
                this._clearFuture();
                this._game = this._game.applyMove(bestMove);
                this._render();

                if (this._game.isGameOver) {
                    await this._showGameOver();
                    return;
                }

                // 人間がパスすべき場合
                if (this._game.mustPass) {
                    await this._handlePassAnimation();
                    this._game = this._game.applyPass();
                    this._render();
                    if (this._game.isGameOver) {
                        await this._showGameOver();
                        return;
                    }
                }
                shouldContinue = true;
            }

            this._refreshHint();
        } catch (error) {
            if (token !== this._aiToken) return;
            console.error('AI move failed', error);
            this._ui.logError('AIの思考に失敗しました。');
        } finally {
            this._isAiThinking = false;
        }

        if (token === this._aiToken && shouldContinue) {
            await this._maybeRunAiTurn();
        }
    }

    /**
     * 強制パスを処理
     * @private
     * @returns {Promise<boolean>} パスが発生したかどうか
     */
    async _handleForcedPasses() {
        let passed = false;
        while (this._game.mustPass) {
            await this._handlePassAnimation();
            this._pushHistory();
            this._clearFuture();
            this._game = this._game.applyPass();
            this._render();
            passed = true;
            if (this._game.isGameOver) break;
        }
        return passed;
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
    async _showGameOver() {
        const { blackCount, whiteCount } = this._game.getResult();
        this._ui.showEndGameModal(
            blackCount,
            whiteCount,
            this._blackPlayerName,
            this._whitePlayerName,
            this._game.record.join(' ')
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
        if (this._game) {
            this._history.push(this._snapshot());
        }
    }

    /**
     * 未来を消去
     * @private
     */
    _clearFuture() {
        this._future = [];
    }

    /**
     * スナップショットを取得
     * @private
     * @returns {HistorySnapshot}
     */
    _snapshot() {
        return {
            game: this._game,
            hintScores: this._hintScores ? [...this._hintScores] : null,
        };
    }

    /**
     * スナップショットから復元
     * @private
     * @param {HistorySnapshot} snapshot
     */
    _restore(snapshot) {
        this._game = snapshot.game;
        this._hintScores = snapshot.hintScores;
        this._render();
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
        if (!this._game) return;

        this._hintService.scheduleRefresh(
            this._game.board,
            this._settingsService.hintLevel,
            this._isAiTurn(),
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
        const board = this._game.board;
        const openingMatch = this._getOpeningMatch();

        return {
            blackBits: board.blackBits,
            whiteBits: board.whiteBits,
            legalMovesBits: board.legalMovesBits,
            nextTurn: board.nextTurn,
            eval: this._hintScores ?? null,
            lastMove: this._game.lastMove,
            currentHumanOpening: openingMatch?.name ?? null,
            humanOpeningNextPosition: openingMatch?.nextPosition ?? null,
        };
    }

    /**
     * 現在の定石マッチングを取得
     * @private
     * @returns {import('./opening-service.js').OpeningMatch | null}
     */
    _getOpeningMatch() {
        if (!this._openingService.isLoaded) return null;
        if (!this._game) return null;

        const selectedOpening = this._settingsService.humanOpening;
        if (selectedOpening === null || selectedOpening === undefined) {
            return null;
        }

        const recordMoves = this._game.record;
        return this._openingService.match(selectedOpening, recordMoves);
    }
}
