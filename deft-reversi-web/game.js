import { Engine } from "./engine-client.js";
import { EventDispatcher } from "./events.js";
import { UI } from "./ui.js";
import {
    applyMoveForTurn,
    bitsToParts,
    countBits,
    getBits,
    isEnd,
    isPass,
    legalMoves,
    sleep,
} from "./utils.js";

const INITIAL_BLACK = 0x0000000810000000n;
const INITIAL_WHITE = 0x0000001008000000n;
const EMPTY_FLIPPING = "0000000000000000000000000000000000000000000000000000000000000000";
const TURN_BLACK = "Black";
const TURN_WHITE = "White";
const letters = "abcdefgh";
const numbers = "12345678";

function positionToStr(pos) {
    const col = pos % 8;
    const row = Math.floor(pos / 8);
    return `${letters[col]}${numbers[row]}`;
}

function oppositeTurn(turnLower) {
    return turnLower === "black" ? TURN_WHITE : TURN_BLACK;
}

function splitBits(bits) {
    const { low, high } = bitsToParts(bits);
    return { low, high };
}

function computeLegal(nextTurn, blackBits, whiteBits) {
    const turn = nextTurn.toLowerCase();
    const player = turn === "black" ? blackBits : whiteBits;
    const opponent = turn === "black" ? whiteBits : blackBits;
    return legalMoves(player, opponent);
}

function buildState({ blackBits, whiteBits, nextTurn, lastMove = null }) {
    const legal = computeLegal(nextTurn, blackBits, whiteBits);
    const { low: black_bits_low, high: black_bits_high } = splitBits(blackBits);
    const { low: white_bits_low, high: white_bits_high } = splitBits(whiteBits);
    const { low: legal_moves_bits_low, high: legal_moves_bits_high } = splitBits(legal);
    return {
        black_bits_low,
        black_bits_high,
        white_bits_low,
        white_bits_high,
        legal_moves_bits_low,
        legal_moves_bits_high,
        next_turn: nextTurn,
        eval: null,
        last_move: lastMove,
        flipping: EMPTY_FLIPPING,
        human_opening_next_position: null,
        current_human_opening: null,
    };
}

function playerOpponentFromState(state) {
    const blackBits = getBits(state, "black");
    const whiteBits = getBits(state, "white");
    const turn = state.next_turn.toLowerCase();
    if (turn === "black") {
        return { player: blackBits, opponent: whiteBits };
    }
    return { player: whiteBits, opponent: blackBits };
}

export class Game {
    constructor() {
        this.eventDispatcher = new EventDispatcher();
        this.ui = new UI(this.eventDispatcher);
        this.queue = Promise.resolve();

        this.enableAi = true;
        this.aiTurn = "white";
        this.aiLevel = 10;
        this.humanOpening = "none";

        this.enableHint = false;
        this.hintLevel = 7;
        this.hintToken = 0;
        this.hintScheduled = false;
        this.lastHintStateKey = null;

        this.blackPlayerName = undefined;
        this.whitePlayerName = undefined;

        this.state = null;
        this.history = [];
        this.future = [];
        this.record = [];

        this.aiReady = false;
        this.isAiThinking = false;
        this.aiToken = 0;

        this.handleEngineMessage = this.handleEngineMessage.bind(this);
        this.setupEngineWorker();
        this.setupEventListeners();
        this.ui.render(undefined, this.blackPlayerName, this.whitePlayerName);

        this.runExclusive(async () => {
            await this.resetState();
        });
    }

    get turnLower() {
        return this.state?.next_turn?.toLowerCase?.() ?? "black";
    }

    get blackBits() {
        return getBits(this.state, "black");
    }

    get whiteBits() {
        return getBits(this.state, "white");
    }

    get isAiToPlay() {
        return this.enableAi && this.turnLower === this.aiTurn;
    }

    setupEngineWorker() {
        if (this.engine) {
            this.engine.worker.removeEventListener("message", this.handleEngineMessage);
            this.engine.worker.terminate();
        }
        this.engine = new Engine();
        this.engine.worker.addEventListener("message", this.handleEngineMessage);
    }

    handleEngineMessage(event) {
        const data = event.data;
        if (data?.type === "ready" && data.ok) {
            this.ui.onAIReady();
            this.aiReady = true;
            if (this.state && this.enableAi) {
                this.runExclusive(async () => {
                    await this.maybeRunAiTurn();
                });
            }
            if (this.enableHint) {
                this.runExclusive(async () => {
                    await this.restartHintIfEnabled(this.hintLevel);
                });
            }
            return;
        }
        if (data?.ok === false) {
            console.error("Worker error:", data.error);
            this.ui.logError(`AIエンジンで問題が発生しました。詳細: ${data.error}`);
        }
    }

    runExclusive(fn) {
        this.queue = this.queue
            .then(fn)
            .catch((error) => {
                console.error("Game error:", error);
                this.ui.logError(`エラーが発生しました: ${error?.message ?? String(error)}`);
            });
        return this.queue;
    }

    async resetState() {
        this.history = [];
        this.future = [];
        this.record = [];
        this.state = buildState({
            blackBits: INITIAL_BLACK,
            whiteBits: INITIAL_WHITE,
            nextTurn: TURN_BLACK,
        });
        this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
        if (this.enableAi && this.aiTurn === "black") {
            await this.maybeRunAiTurn();
        }
        this.scheduleHintRefresh(this.hintLevel);
    }

    snapshot() {
        return {
            state: structuredClone(this.state),
            record: [...this.record],
        };
    }

    restore(snapshot) {
        this.state = snapshot.state;
        this.record = [...snapshot.record];
        this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
        this.scheduleHintRefresh(this.hintLevel);
    }

    pushHistory() {
        if (this.state) this.history.push(this.snapshot());
    }

    clearFuture() {
        this.future = [];
    }

    async showGameOver() {
        const blackCount = countBits(this.blackBits);
        const whiteCount = countBits(this.whiteBits);
        const record = this.record.join(" ");
        this.ui.showEndGameModal(
            blackCount,
            whiteCount,
            this.blackPlayerName,
            this.whitePlayerName,
            record,
        );
    }

    async handlePassAnimation() {
        this.ui.drawPassMessage();
        await sleep(600);
    }

    commitState({ blackBits, whiteBits, nextTurn, lastMove, recordEntry }) {
        this.pushHistory();
        this.clearFuture();
        if (recordEntry) this.record.push(recordEntry);
        this.state = buildState({ blackBits, whiteBits, nextTurn, lastMove });
        this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
        this.scheduleHintRefresh(this.hintLevel);
    }

    clearHint() {
        if (!this.state) return;
        this.state.eval = null;
    }

    cancelHintJob() {
        this.hintToken += 1;
        this.hintScheduled = false;
        this.lastHintStateKey = null;
    }

    currentStateKey() {
        if (!this.state) return null;
        const {
            black_bits_low,
            black_bits_high,
            white_bits_low,
            white_bits_high,
            next_turn,
        } = this.state;
        return `${black_bits_low}:${black_bits_high}:${white_bits_low}:${white_bits_high}:${next_turn}`;
    }

    scheduleHintRefresh(level = this.hintLevel) {
        if (!this.enableHint) return;
        if (!this.aiReady) return;
        if (!this.state) return;
        // AIの手番中はヒント不要（思考中フラグではなく、手番で判定する）
        if (this.isAiToPlay) return;
        if (this.hintScheduled) return;
        if (this.lastHintStateKey !== null && this.lastHintStateKey === this.currentStateKey()) return;
        this.hintScheduled = true;
        queueMicrotask(() => {
            this.hintScheduled = false;
            this.startHintJob(level);
        });
    }

    restartHintIfEnabled(level = this.hintLevel) {
        if (!this.enableHint) return;
        this.scheduleHintRefresh(level);
    }

    startHintJob(level = this.hintLevel) {
        if (!this.enableHint) return;
        if (!this.aiReady) return;
        if (!this.state) return;
        if (this.isAiToPlay) return;

        this.cancelHintJob();
        const token = this.hintToken;
        const targetKey = this.currentStateKey();
        this.lastHintStateKey = targetKey;

        const legalBits = getBits(this.state, "legal_moves");
        if (legalBits === null || legalBits === undefined || legalBits === 0n) {
            this.clearHint();
            this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
            return;
        }

        const turn = this.turnLower;
        const playerBits = turn === "black" ? this.blackBits : this.whiteBits;
        const opponentBits = turn === "black" ? this.whiteBits : this.blackBits;

        const moves = [];
        for (let i = 0; i < 64; i++) {
            if ((legalBits >> BigInt(i)) & 1n) moves.push(i);
        }

        const scores = Array(64).fill(null);
        this.state.eval = scores;
        this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);

        const sortByScoreDesc = () =>
            moves.sort((a, b) => {
                const sa = scores[a];
                const sb = scores[b];
                if (sa === null || sa === undefined) return 1;
                if (sb === null || sb === undefined) return -1;
                if (sa !== sb) return sb - sa;
                return a - b;
            });

        const shouldCancel = () => {
            if (token !== this.hintToken) return true;
            if (targetKey !== this.currentStateKey()) return true;
            return false;
        };

        const calcLevel = async (lv) => {
            sortByScoreDesc();
            for (const pos of moves) {
                if (shouldCancel()) return false;
                try {
                    const { nextPlayer, nextOpponent } = applyMoveForTurn(playerBits, opponentBits, pos);
                    const nextPlayerBits = nextOpponent;
                    const nextOpponentBits = nextPlayer;
                    const { low: player_bits_low, high: player_bits_high } = bitsToParts(nextPlayerBits);
                    const { low: opponent_bits_low, high: opponent_bits_high } = bitsToParts(nextOpponentBits);
                    const result = await this.engine.solveTurn({
                        player_bits_low,
                        player_bits_high,
                        opponent_bits_low,
                        opponent_bits_high,
                        aiLevel: lv,
                    });
                    if (shouldCancel()) return false;
                    const evalScore = typeof result?.eval === "number" ? result.eval : 0;
                    scores[pos] = -evalScore;
                    this.state.eval = scores;
                    this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
                    await new Promise(requestAnimationFrame);
                } catch (error) {
                    if (shouldCancel()) return false;
                    console.error("Hint eval failed", error);
                }
            }
            return true;
        };

        const run = async () => {
            for (let lv = 1; lv <= level; lv++) {
                const ok = await calcLevel(lv);
                if (!ok) return;
            }
            // Do not call cancelHintJob() here:
            // a newer hint job may have started while this one was running.
            if (token === this.hintToken && targetKey === this.currentStateKey()) {
                this.lastHintStateKey = targetKey;
            }
        };

        run();
    }

    applyPassForCurrentTurn() {
        const nextTurn = oppositeTurn(this.turnLower);
        this.commitState({
            blackBits: this.blackBits,
            whiteBits: this.whiteBits,
            nextTurn,
            lastMove: this.state.last_move,
            recordEntry: "pass",
        });
    }

    hasToPass() {
        const { player, opponent } = playerOpponentFromState(this.state);
        return isPass(player, opponent);
    }

    isGameOver() {
        const { player, opponent } = playerOpponentFromState(this.state);
        return isEnd(player, opponent);
    }

    async handleForcedPasses() {
        let passed = false;
        while (this.hasToPass()) {
            await this.handlePassAnimation();
            this.applyPassForCurrentTurn();
            passed = true;
            if (this.isGameOver()) break;
        }
        return passed;
    }

    /**
     * 現在のプレーヤーの着手を適用し、ゲームの状態を更新します。
     * この関数は、指定された位置に石を置いた後の新しい盤面を計算し、
     * ゲーム履歴に新しい状態を記録（コミット）します。
     * ゲームオーバーのチェックやパスの処理は行いません。
     * @param {number} position 石を置く盤上の位置 (0-63)。
     * @returns {void}
     */
    applyMoveForCurrentTurn(position) {
        const { player, opponent } = playerOpponentFromState(this.state);
        const turn = this.turnLower;
        const { nextPlayer, nextOpponent } = applyMoveForTurn(player, opponent, position);
        const nextTurn = oppositeTurn(turn);

        const blackBits = turn === "black" ? nextPlayer : nextOpponent;
        const whiteBits = turn === "black" ? nextOpponent : nextPlayer;

        this.commitState({
            blackBits,
            whiteBits,
            nextTurn,
            lastMove: position,
            recordEntry: positionToStr(position),
        });
    }

    async applyMove(position) {
        this.applyMoveForCurrentTurn(position);

        if (this.isGameOver()) {
            await this.showGameOver();
            return;
        }
        if (this.hasToPass()) {
            await this.handlePassAnimation();
            this.applyPassForCurrentTurn();
            if (this.isGameOver()) {
                await this.showGameOver();
            }
        }

        await this.restartHintIfEnabled(this.hintLevel);
    }

    async startAiTurn() {
        if (!this.aiReady) return;
        if (this.isAiThinking) return;

        if (this.isGameOver()) {
            await this.showGameOver();
            return;
        }

        const passed = await this.handleForcedPasses();
        if (this.isGameOver()) {
            await this.showGameOver();
            return;
        }

        if (passed) {
            await this.maybeRunAiTurn();
            return;
        }

        if (!this.isAiToPlay) return;

        // AIの手番中はヒント不要なので消しておく
        if (this.enableHint) {
            this.cancelHintJob();
            this.clearHint();
            this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
        }

        this.isAiThinking = true;
        const token = ++this.aiToken;
        let shouldContinue = false;
        this.ui.logInfo("AIが思考中です...");
        try {
            const turn = this.turnLower;
            const playerBits = turn === "black" ? this.blackBits : this.whiteBits;
            const opponentBits = turn === "black" ? this.whiteBits : this.blackBits;
            const { low: player_bits_low, high: player_bits_high } = bitsToParts(playerBits);
            const { low: opponent_bits_low, high: opponent_bits_high } = bitsToParts(opponentBits);

            const result = await this.engine.solveTurn({
                player_bits_low,
                player_bits_high,
                opponent_bits_low,
                opponent_bits_high,
                aiLevel: this.aiLevel,
            });
            if (token !== this.aiToken) return;
            console.log("AI solveTurn result:", result);
            const bestMovePos = (() => {
                const low = result?.best_move_low ?? 0;
                const high = result?.best_move_high ?? 0;
                const mask = (BigInt(high >>> 0) << 32n) | BigInt(low >>> 0);
                if (mask === 0n) return -1;
                let pos = 0;
                let m = mask;
                while ((m & 1n) === 0n) {
                    m >>= 1n;
                    pos++;
                }
                return pos;
            })();
            if (typeof bestMovePos !== "number" || bestMovePos < 0) {
                this.ui.logInfo("AIはパスします。");
                await this.handlePassAnimation();
                this.applyPassForCurrentTurn();
                shouldContinue = true;
                return;
            }
            await this.applyMove(bestMovePos);
            shouldContinue = true;
        } catch (error) {
            if (token !== this.aiToken) return;
            console.error("AI move failed", error);
            this.ui.logError("AIの思考に失敗しました。");
        } finally {
            this.isAiThinking = false;
        }

        if (token === this.aiToken && shouldContinue) {
            await this.maybeRunAiTurn();
        }
    }

    async maybeRunAiTurn() {
        if (!this.enableAi) return;
        await this.startAiTurn();
    }

    async handleHumanMove(position) {
        if (this.isAiToPlay) {
            this.ui.logInfo("AIの手番です。お待ちください。");
            return;
        }
        if (this.isGameOver()) {
            await this.showGameOver();
            return;
        }
        const passed = await this.handleForcedPasses();
        if (this.isGameOver()) {
            await this.showGameOver();
            return;
        }

        if (passed) {
            await this.maybeRunAiTurn();
            return;
        }

        try {
            await this.applyMove(position);
            await this.maybeRunAiTurn();
        } catch (error) {
            console.error("Human move failed", error);
            this.ui.logError("その手は打てません。");
        }
    }

    setupEventListeners() {
        this.eventDispatcher.addEventListener("boardClick", (position) =>
            this.runExclusive(async () => {
                await this.handleHumanMove(position);
            }),
        );

        this.eventDispatcher.addEventListener("newGameClick", () =>
            this.runExclusive(async () => {
                this.aiReady = false;
                this.isAiThinking = false;
                this.aiToken += 1;
                this.setupEngineWorker();
                this.cancelHintJob();
                await this.resetState();
            }),
        );

        this.eventDispatcher.addEventListener("doOverClick", () =>
            this.runExclusive(async () => {
                if (this.history.length === 0) return;
                this.future.push(this.snapshot());
                const snapshot = this.history.pop();
                this.restore(snapshot);
                this.cancelHintJob();
                if (this.enableHint) await this.restartHintIfEnabled(this.hintLevel);
            }),
        );

        this.eventDispatcher.addEventListener("redoClick", () =>
            this.runExclusive(async () => {
                if (this.future.length === 0) return;
                this.history.push(this.snapshot());
                const snapshot = this.future.pop();
                this.restore(snapshot);
                this.cancelHintJob();
                if (this.enableHint) await this.restartHintIfEnabled(this.hintLevel);
            }),
        );

        this.eventDispatcher.addEventListener("switchShowEvalClick", () =>
            this.runExclusive(async () => {
                this.enableHint = !this.enableHint;
                this.cancelHintJob();
                if (this.enableHint) {
                    await this.restartHintIfEnabled(this.hintLevel);
                } else {
                    this.clearHint();
                    this.ui.render(this.state, this.blackPlayerName, this.whitePlayerName);
                }
            }),
        );

        this.eventDispatcher.addEventListener("deepHintClick", () => {
            const depth = Number(
                window.prompt(
                    "現在の盤面のヒントをより深く計算します。\nAIのレベル(1 ~ 24)を入力してください。",
                    String(this.hintLevel),
                ),
            );
            if (Number.isInteger(depth) && 1 <= depth && depth <= 24) {
                this.runExclusive(async () => {
                    this.enableHint = true;
                    this.hintLevel = depth;
                    this.cancelHintJob();
                    await this.restartHintIfEnabled(depth);
                });
            } else {
                this.ui.logError("無効な入力です。AIのレベル(1 ~ 24)を整数値で入力してください。");
            }
        });

        this.eventDispatcher.addEventListener("setAILevel", (lv) => {
            this.aiLevel = lv;
        });
        this.eventDispatcher.addEventListener("setEnableAI", (f) => {
            this.enableAi = f;
        });
        this.eventDispatcher.addEventListener("setAITurn", (aiTurn) => {
            this.aiTurn = aiTurn;
        });
        this.eventDispatcher.addEventListener("setPlayerName", (blackPlayerName, whitePlayerName) => {
            this.blackPlayerName = blackPlayerName;
            this.whitePlayerName = whitePlayerName;
        });
        this.eventDispatcher.addEventListener("setHumanOpening", (humanOpening) =>
            this.runExclusive(async () => {
                this.humanOpening = humanOpening;
            }),
        );
    }
}
