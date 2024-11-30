import { BoardUI } from "./board.js";
import { StatusUI } from "./status.js";

const sleep = (time) => new Promise((resolve) => setTimeout(resolve, time));//timeはミリ秒

class EventDispatcher {
    constructor() {
        this.listeners = {};
    }

    addEventListener(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
    }

    dispatchEvent(event, ...data) {
        if (this.listeners[event]) {
            this.listeners[event].forEach(callback => callback(...data));
        }
    }
}

class UI {
    constructor(eventDispatcher) {
        this.initModalWindow();
        this.initEndGameModalWindow();
        this.cv = document.getElementById("cv");
        this.ctx = this.cv.getContext("2d");

        this.scale = this.scale.bind(this);
        this.scale();
        window.addEventListener('resize', this.scale);

        this.board = new BoardUI();
        this.statusUI = new StatusUI();

        this.eventDispatcher = eventDispatcher;
        this.cv.addEventListener('click', this.handleClick.bind(this));
        this.aiReady = false;
    }

    onAIReady() {
        const startButton = document.getElementById('start-button');
        startButton.textContent = "Game Start !";
        this.aiReady = true;
    }

    initModalWindow() {
        const aiToggle = document.getElementById('ai-toggle');
        const aiLevelSetting = document.getElementById('ai-level-setting');
        const turnSetting = document.getElementById('turn-setting');
        const modal = document.getElementById('modal');
        const startButton = document.getElementById('start-button');
        const aiLevelSlider = document.getElementById('ai-level-slider');
        const levelDisplay = document.getElementById('level-display');
        this.setLevelDisplay = (lv) => {
            if (lv > 20) {
                levelDisplay.textContent = "Lv. " + aiLevelSlider.value + " (非推奨)";
            } else {
                levelDisplay.textContent = "Lv. " + aiLevelSlider.value;
            }
        }
        const firstButton = document.getElementById('first-button');
        const secondButton = document.getElementById('second-button');

        if (!this.aiReady) startButton.textContent = "Now Loading...";

        const savedSettingsString = localStorage.getItem('gameSettings');
        if (savedSettingsString !== null){
            // ローカルストレージから設定を復元
            const savedSettings = JSON.parse(localStorage.getItem('gameSettings')) || {
                aiEnabled: true,
                aiLevel: 5,
                aiTurn: "white",
            };
    
            aiToggle.classList.toggle('active', savedSettings.aiEnabled);
            aiLevelSetting.classList.toggle('hidden', !savedSettings.aiEnabled);
            turnSetting.classList.toggle('hidden', !savedSettings.aiEnabled);
            aiLevelSlider.value = savedSettings.aiLevel;
            this.setLevelDisplay(savedSettings.aiLevel);
    
            if (savedSettings.aiTurn === "white") {
                firstButton.classList.add('active');
                secondButton.classList.remove('active');
            } else if (savedSettings.aiTurn === "black") {
                secondButton.classList.add('active');
                firstButton.classList.remove('active');
            }
        } else {
            /*
            デフォルト設定
                AIが打つ
                レベル: 1
                先攻
            */
            modal.style.height = 'auto';
            aiToggle.classList.toggle('active', true);
            aiLevelSetting.classList.toggle('hidden', false);
            turnSetting.classList.toggle('hidden', false);
            firstButton.classList.add('active');
            secondButton.classList.remove('active');
            aiLevelSlider.value = 1;
            this.setLevelDisplay(1);
        }



        // Update level display in real-time
        aiLevelSlider.addEventListener('input', () => {
            this.setLevelDisplay(aiLevelSlider.value);
        });

        // Toggle AI setting
        aiToggle.addEventListener('click', () => {
            aiToggle.classList.toggle('active');
            const isAiEnabled = aiToggle.classList.contains('active');

            aiLevelSetting.classList.toggle('hidden', !isAiEnabled);
            turnSetting.classList.toggle('hidden', !isAiEnabled);

            modal.style.height = isAiEnabled ? 'auto' : '250px';
        });

        // Toggle first/second button
        firstButton.addEventListener('click', () => {
            firstButton.classList.add('active');
            secondButton.classList.remove('active');
        });
        secondButton.addEventListener('click', () => {
            secondButton.classList.add('active');
            firstButton.classList.remove('active');
        });

        // Start the game
        startButton.addEventListener('click', (() => {
            if (!this.aiReady) return;
            const aiTurn = (() => {
                if (firstButton.classList.contains('active')) {
                    return "white";
                } else if (secondButton.classList.contains('active')) {
                    return "black";
                }
            })();

            const settings = {
                aiEnabled: aiToggle.classList.contains('active'),
                aiLevel: parseInt(aiLevelSlider.value),
                aiTurn: aiTurn,
            };

            if (settings.aiEnabled) {
                if (settings.aiTurn == "white") {
                    this.blackPlayerName = "あなた";
                    this.whitePlayerName = `AI Lv ${settings.aiLevel}`;
                } else {
                    this.blackPlayerName = `AI Lv ${settings.aiLevel}`;
                    this.whitePlayerName = "あなた";
                }
            } else {
                this.blackPlayerName = "先攻";
                this.whitePlayerName = "後攻";
            }

            localStorage.setItem('gameSettings', JSON.stringify(settings));
            this.eventDispatcher.dispatchEvent('setEnableAI', settings.aiEnabled);
            this.eventDispatcher.dispatchEvent('setAILevel', settings.aiLevel);
            this.eventDispatcher.dispatchEvent('setAITurn', settings.aiTurn);
            this.eventDispatcher.dispatchEvent('setPlayerName', this.blackPlayerName, this.whitePlayerName);
            this.eventDispatcher.dispatchEvent("draw");

            // モーダルをフェードアウト
            const modalOverlay = document.getElementById('modal-overlay');
            modalOverlay.classList.add('fade-out');

            // フェードアウトが完了した後に非表示にする
            modalOverlay.addEventListener('animationend', () => {
                modalOverlay.style.display = 'none';
            }, { once: true });
        }).bind(this));
    }

    initEndGameModalWindow(){
        this.endGameModalOverlay = document.getElementById('end-game-modal-overlay');
        this.gameHistory = document.getElementById('game-history');
        this.copyHistoryButton = document.getElementById('copy-history-button');
        this.closeEndGameModal = document.getElementById('close-end-game-modal');

        // 棋譜をコピーする
        this.copyHistoryButton.addEventListener('click', () => {
            navigator.clipboard.writeText(this.gameHistory.value).then(() => {
                alert("棋譜がコピーされました！");
            });
        });

        // モーダルを閉じる
        this.closeEndGameModal.addEventListener('click', () => {
            this.endGameModalOverlay.style.display = 'none';
        });
    }

    showEndGameModal(blackScore, whiteScore, blackPlayerName, whitePlayerName, history) {
        const endGameModalOverlay = document.getElementById('end-game-modal-overlay');
        const blackScoreElement = document.getElementById('black-score');
        const whiteScoreElement = document.getElementById('white-score');
        const winnerText = document.getElementById('winner-text');
        const gameHistory = document.getElementById('game-history');
        
        // スコアの設定
        blackScoreElement.textContent = blackScore;
        whiteScoreElement.textContent = whiteScore;

        // 勝者の設定
        if (blackScore > whiteScore) {
            winnerText.textContent = `${blackPlayerName} の勝利!`;
            winnerText.style.color = "#FFD700";
        } else if (whiteScore > blackScore) {
            winnerText.textContent = `${whitePlayerName} の勝利!`;
            winnerText.style.color = "#FFD700";
        } else {
            winnerText.textContent = "引き分け!";
            winnerText.style.color = "#4CAF50";
        }

        // 棋譜の設定
        gameHistory.value = history;

        // モーダルの表示
        endGameModalOverlay.style.display = 'flex';
    }


    hideEndGameModal() {
        this.endGameModalOverlay.style.display = 'none';
    }


    showModalWindow() {
        const modalOverlay = document.getElementById('modal-overlay');
        modalOverlay.classList.remove('fade-out');
        modalOverlay.style.display = '';
    }
    scale() {
        // コンストラクタで、thisがbindされています。
        const scale = Math.min(window.innerWidth / this.cv.width, window.innerHeight / this.cv.height);

        this.cv.style.width = `${this.cv.width * scale}px`;
        this.cv.style.height = `${this.cv.height * scale}px`;
    }

    drawPassMessage() {
        const centerX = this.cv.width / 2;
        const centerY = this.cv.height  / 2;

        this.ctx.fillStyle = "rgba(0, 0, 0, 0.8)"; // 半透明の背景
        this.ctx.fillRect(centerX - 100, centerY - 50, 200, 100);

        this.ctx.font = "36px Arial";
        this.ctx.fillStyle = "white";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";
        this.ctx.fillText("パス", centerX, centerY);
    }

    handleClick(event) {
        const rect = this.cv.getBoundingClientRect();

        const scaleX = this.cv.clientWidth / this.cv.width;
        const scaleY = this.cv.clientHeight / this.cv.height;

        const x = (event.clientX - rect.left) / scaleX;
        const y = (event.clientY - rect.top) / scaleY;

        // boardClick
        const position = this.board.getBoardPosition(x, y);
        if (position !== undefined) {
            this.eventDispatcher.dispatchEvent('boardClick', position);
            return;
        }

        const clickedButton = this.statusUI.isClickButton(x, y);
        if (clickedButton !== undefined) {
            switch (clickedButton) {
                case 0:
                    this.eventDispatcher.dispatchEvent('newGameClick');
                    this.showModalWindow();
                    break;
                case 1:
                    this.eventDispatcher.dispatchEvent('doOverClick');
                    break;
                case 2:
                    this.eventDispatcher.dispatchEvent('switchShowEvalClick');
                    break;
                default:
                    break;
            }
        }
    }

    render(status, blackPlayerName, whitePlayerName) {
        this.ctx.clearRect(0, 0, this.cv.width, this.cv.height);
        this.board.update(status);
        this.statusUI.update(status, blackPlayerName, whitePlayerName);
    }


}

class WorkerWrapper {
    constructor(workerFile) {
        this.worker = new Worker(workerFile, { type: 'module' });
        this.setWorkerWrapper();
        this.pendingRequests = new Map();
        this.generateRequestId = () => { return `${Date.now()}-${Math.random()}` };
    }
    setWorkerWrapper() {
        this.worker.addEventListener('message', (event) => {
            const { payload, requestId } = event.data;
            const pending = this.pendingRequests.get(requestId);
            if (pending) {
                pending.resolve(payload);
                this.pendingRequests.delete(requestId);
            }
        });
    }
    sendRequest(type, ...payload) {
        return new Promise((resolve, reject) => {
            const requestId = this.generateRequestId();
            this.pendingRequests.set(requestId, { resolve, reject });
            this.worker.postMessage({ type, payload, requestId });
        });
    }
}

class Engine extends WorkerWrapper {
    constructor() {
        super('engine.js');
        this.initializeMethods([
            'getState',
            'put',
            'aiPut',
            'undo',
            'redo',
            'isLegalMove',
            'isPass',
            'pass',
            'isEnd',
            'getRecord',
            'newGame',
            'doOver',
        ]);
    }

    initializeMethods(methods) {
        methods.forEach((method) => {
            this[method] = (...payload) => this.sendRequest(method, ...payload);
        });
    }
}

export class Game {
    constructor() {
        this.engine = new Engine();
        this.eventDispatcher = new EventDispatcher();
        this.ui = new UI(this.eventDispatcher);

        this.engine.worker.addEventListener('message', async (event) => {
            if (event.data == "ready") {
                this.draw_no_score();
                this.ui.onAIReady();
                return;
            }
        });

        this.isThinking = false;
        this.enableAi = true;
        this.putAILv = 10;
        this.drawId = 0;
        this.draw_move_scores_lv = 8;
        this.enableDrawEval = false;
        this.aiTurn = "white";
        this.setupEventListeners();
        this.ui.render(undefined, this.blackPlayerName, this.whitePlayerName);
    }

    setupEventListeners() {
        this.eventDispatcher.addEventListener('boardClick', this.boardClick.bind(this));
        this.eventDispatcher.addEventListener('newGameClick', this.newGameClick.bind(this));
        this.eventDispatcher.addEventListener('doOverClick', this.doOverClick.bind(this));
        this.eventDispatcher.addEventListener('switchShowEvalClick', this.switchEnableDrawEvalClick.bind(this));

        this.eventDispatcher.addEventListener('draw', this.draw.bind(this));
        this.eventDispatcher.addEventListener('setAILevel', ((lv) => {this.putAILv = lv}).bind(this));
        this.eventDispatcher.addEventListener('setEnableAI', ((f) => { this.enableAi = f }).bind(this));

        this.eventDispatcher.addEventListener('setAITurn', ((aiTurn) => { 
            if (aiTurn == "black" && this.enableAi) { 
                this.isThinking = true;
                setTimeout((() => {
                    this.aiPut().then(async () => {
                        await this.draw();
                        this.isThinking = false;
                    });
                }).bind(this), 600);
            }
        }).bind(this));
        this.eventDispatcher.addEventListener('setPlayerName', ((black, white) => {
            this.blackPlayerName = black;
            this.whitePlayerName = white;
        }).bind(this));
    }

    async draw_no_score() {
        this.drawId++;
        this.ui.render(await this.engine.getState(null), this.blackPlayerName, this.whitePlayerName);
    }

    async waitAI() {
        while (this.isThinking) {
            await sleep(100);
        }
    }

    async aiPut() {
        await this.engine.aiPut(this.putAILv);
        this.draw_no_score();
        if (await this.engine.isEnd()) { return; }
        if (await this.engine.isPass()) {
            this.ui.drawPassMessage();
            await sleep(1000);
            await this.engine.pass();
            await this.aiPut();
        }
    }

    async put(position) {
        if (!await this.engine.isLegalMove(position)) {
            return;
        }
        await this.engine.put(position);

        this.draw_no_score();

        if (await this.engine.isEnd()) {return;}
        if (await this.engine.isPass()) {
            this.ui.drawPassMessage();
            await sleep(1000);
            await this.engine.pass();
            return;
        }

        if (this.enableAi == true) {
            await this.aiPut();
        }
    }

    async draw() {
        this.drawId++;
        if (this.enableDrawEval) {
            const lv = this.draw_move_scores_lv;
            const id = this.drawId;
            const step = 3;
            const start = lv % step == 0 ? step : lv % step;
            for (let i = start; i <= lv; i += step) {
                console.log("draw");
                await this.waitAI();
                if (id != this.drawId) break;
                this.ui.render(await this.engine.getState(i), this.blackPlayerName, this.whitePlayerName);
                await new Promise(resolve => setTimeout(resolve, 0));
            }
        }
        else {
            this.ui.render(await this.engine.getState(null), this.blackPlayerName, this.whitePlayerName);
        }
    }
    async endGame() {
        let count = (str) => {
            let c = 0;
            for (var i = 0; i < str.length; i++) {
                if (str[i] == "1") c++;
            }
            return c;
        };
        const status = await this.engine.getState(null);
        const blackCount = count(status.black);
        const whiteCount = count(status.white);
        this.ui.showEndGameModal(blackCount, whiteCount, this.blackPlayerName, this.whitePlayerName, await this.engine.getRecord());
    }

    async boardClick(position) {
        if (this.isThinking) {
            console.log("AI is Thinking !");
            return;
        }
        this.isThinking = true;
        this.put(position).then(async () => { 
            console.log(await this.engine.getRecord());
            if (await this.engine.isEnd()) {
                this.endGame();
            } else {
                this.draw();
            }

            this.isThinking = false;
        });
    }
    async doOverClick() {
        await this.waitAI();
        this.isThinking = true;
        if (this.enableAi){
            if (this.aiTurn == "white"){
                const record = await this.engine.getRecord();
                if (record.length == 2){
                    this.isThinking = false;
                    return;
                }
            }
            await this.engine.doOver();
        } else {
            await this.engine.undo();
        }
        this.isThinking = false;
        this.draw();
    }

    async newGameClick() {
        while (this.isThinking) {
            // AIが応答するまで、何もない盤面を出し続ける
            await sleep(20);
            this.ui.render(undefined, this.blackPlayerName, this.whitePlayerName);
        }

        this.isThinking = true;
        await this.engine.newGame();
        this.isThinking = false;
        this.drawId = 0;
        this.draw();
    }

    async switchEnableDrawEvalClick() {
        this.enableDrawEval = !this.enableDrawEval;
        await this.draw();
    }

}