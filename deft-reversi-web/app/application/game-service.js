/**
 * @fileoverview GameService - ゲーム進行を統括するアプリケーションサービス
 */

import { Game } from '../domain/game.js';
import { turnToLower } from '../domain/types.js';
import { AiEngine } from '../infrastructure/ai-engine.js';
import { HintService } from './hint-service.js';
import { SettingsService } from './settings-service.js';
import { OpeningService } from './opening-service.js';
import { EventDispatcher } from '../events.js';
import { UI } from '../ui/ui.js';
import { sleep } from '../utils/time.js';

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
 * @property {import('../domain/game.js').GameSnapshot} game
 * @property {(number | null)[] | null} hintScores
 */

/**
 * ゲーム進行を統括するアプリケーションサービス
 */
export class GameService {
    /**
     * @param {Object} [dependencies] オプショナルな依存関係
     * @param {import('../events.js').EventDispatcher} [dependencies.eventDispatcher]
     * @param {Object} [dependencies.ui]
     */
    constructor(dependencies = {}) {
        // 依存関係が渡されなければ内部で生成
        /** @private */
        this._eventDispatcher = dependencies.eventDispatcher ?? new EventDispatcher();
        /** @private */
        this._settingsService = new SettingsService();
        /** @private */
        this._ui =
            dependencies.ui ??
            new UI(this._eventDispatcher, {
                aiEnabled: this._settingsService.enableAi,
                aiLevel: this._settingsService.aiLevel,
                aiTurn: this._settingsService.aiTurn,
                humanOpening: this._settingsService.humanOpening,
            });

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

        this._setupEventListeners();
        this._initializeEngine();
        this._ui.render(undefined, this._blackPlayerName, this._whitePlayerName);

        this._runExclusive(async () => {
            await this._startNewGame();
        });
    }

    // === 公開API ===

    /**
     * SettingsServiceへのアクセス
     * @returns {SettingsService}
     */
    get settings() {
        return this._settingsService;
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

    /** @private */
    _setupEventListeners() {
        this._eventDispatcher.addEventListener('boardClick', (position) =>
            this._runExclusive(() => this._handleHumanMove(position))
        );

        this._eventDispatcher.addEventListener('newGameClick', () =>
            this._runExclusive(async () => {
                this._aiToken += 1;
                this._isAiThinking = false;
                this._hintService.cancel();
                this._aiEngine.terminate();
                this._initializeEngine();
                await this._startNewGame();
            })
        );

        this._eventDispatcher.addEventListener('doOverClick', () =>
            this._runExclusive(() => this._undo())
        );

        this._eventDispatcher.addEventListener('redoClick', () =>
            this._runExclusive(() => this._redo())
        );

        this._eventDispatcher.addEventListener('switchShowEvalClick', () =>
            this._runExclusive(() => this._toggleHint())
        );

        this._eventDispatcher.addEventListener('deepHintClick', () => {
            const depth = Number(
                window.prompt(
                    '現在の盤面のヒントをより深く計算します。\nAIのレベル(1 ~ 24)を入力してください。',
                    String(this._settingsService.hintLevel)
                )
            );
            if (Number.isInteger(depth) && depth >= 1 && depth <= 24) {
                this._runExclusive(() => this._deepHint(depth));
            } else {
                this._ui.logError('無効な入力です。AIのレベル(1 ~ 24)を整数値で入力してください。');
            }
        });

        this._eventDispatcher.addEventListener('setAILevel', (lv) => {
            this._settingsService.aiLevel = lv;
            this._settingsService.save();
        });

        this._eventDispatcher.addEventListener('setEnableAI', (f) => {
            this._settingsService.enableAi = f;
            this._settingsService.save();
        });

        this._eventDispatcher.addEventListener('setAITurn', (turn) => {
            this._settingsService.aiTurn = turn;
            this._settingsService.save();
        });

        this._eventDispatcher.addEventListener('setPlayerName', (black, white) => {
            this._blackPlayerName = black;
            this._whitePlayerName = white;
        });

        this._eventDispatcher.addEventListener('setHumanOpening', (opening) => {
            this._settingsService.humanOpening = opening;
            this._settingsService.save();
        });
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
        this._history = [];
        this._future = [];
        this._hintScores = null;
        this._game = Game.newGame();
        this._render();

        if (this._isAiTurn()) {
            await this._maybeRunAiTurn();
        }
        this._refreshHint();
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

        this._ui.logInfo('AIが思考中です...');

        try {
            const board = this._game.board;
            const { bestMove, eval: evalScore } = await this._aiEngine.solveTurn(
                board.playerBits,
                board.opponentBits,
                this._settingsService.aiLevel
            );

            if (token !== this._aiToken) return;

            console.log('AI solveTurn result:', { bestMove, eval: evalScore });

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
        await sleep(600);
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
            this._game.record.toString()
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
            game: this._game.snapshot(),
            hintScores: this._hintScores ? [...this._hintScores] : null,
        };
    }

    /**
     * スナップショットから復元
     * @private
     * @param {HistorySnapshot} snapshot
     */
    _restore(snapshot) {
        this._game = Game.fromSnapshot(snapshot.game);
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

        const recordMoves = this._game.record.moves;
        return this._openingService.match(selectedOpening, recordMoves);
    }
}
