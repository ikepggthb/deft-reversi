import { Engine } from "./engine-client.js";
import { EventDispatcher } from "./events.js";
import { UI } from "./ui.js";
import { countBits, sleep, getBits } from "./utils.js";

export class Game {
    constructor() {
        this.engine = new Engine();
        this.eventDispatcher = new EventDispatcher();
        this.ui = new UI(this.eventDispatcher);

        this.engine.worker.addEventListener('message', async (event) => {
            const data = event.data;
            if (data?.type === "ready" && data.ok) {
                await this.draw_force();
                this.ui.onAIReady();
                return;
            }
            if (data?.ok === false) {
                console.error("Worker error:", data.error);
                this.ui.logError(`AIエンジンで問題が発生しました。詳細: ${data.error}`);
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
        this.eventDispatcher.addEventListener('newGameClick', this.newGame.bind(this));
        this.eventDispatcher.addEventListener('doOverClick', this.doOverClick.bind(this));
        this.eventDispatcher.addEventListener('switchShowEvalClick', this.switchEnableDrawEvalClick.bind(this));
        this.eventDispatcher.addEventListener('redoClick', this.redoClick.bind(this))
        this.eventDispatcher.addEventListener('deepHintClick', this.deepHintClick.bind(this))

        this.eventDispatcher.addEventListener('draw', this.draw.bind(this));
        this.eventDispatcher.addEventListener('setAILevel', ((lv) => {this.putAILv = lv}).bind(this));
        this.eventDispatcher.addEventListener('setEnableAI', ((f) => { this.enableAi = f }).bind(this));

        this.eventDispatcher.addEventListener('setAITurn', ((aiTurn) => { 
            this.aiTurn = aiTurn;
        }).bind(this));
        this.eventDispatcher.addEventListener('setPlayerName', ((black, white) => {
            this.blackPlayerName = black;
            this.whitePlayerName = white;
        }).bind(this));
        this.eventDispatcher.addEventListener('setHumanOpening', (async (f) => { 
            this.isThinking = true;
            if (f !== "none") {
                const name_index =  Number(f);
                if (!isNaN(name_index)) {
                    try {
                        await this.engine.setHumanOpening(f);
                    } catch (error) {
                        console.error("Failed to set human opening", error);
                        this.ui.logError("定石設定に失敗しました。");
                    }
                }
            }
            this.isThinking = false;
         }).bind(this));
        
    }

    async draw_force() {
        // this.isThinking == true の間に使用
        // await 推奨
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
        await this.draw_force();
        if (await this.engine.isEnd()) { return; }
        if (await this.engine.isPass()) {
            this.ui.drawPassMessage();
            await sleep(1000);
            await this.engine.pass();
            await this.aiPut();
        }
    }

    async put(position) {
        if (await this.engine.isEnd()) { return; }
        if (await this.engine.isPass()) {
            this.ui.drawPassMessage();
            await sleep(1000);
            await this.engine.pass();
            return;
        }
        const status = await this.engine.getState(null);
        if (this.enableAi == true && status.next_turn.toLowerCase() == this.aiTurn) {
            return await this.aiPut();
        }


        if (!await this.engine.isLegalMove(position)) {
            return;
        }
        await this.engine.put(position);

        await this.draw_force();

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
        // this.isThinking == true の間に、awaitをつけてこの関数を実行すると、デットロックする可能性がある
        // this.isThinking == true の間は、"await draw_force()"を使用
        if (this.enableDrawEval) {
            this.drawId++;
            const lv = this.draw_move_scores_lv;
            const id = this.drawId;
            const step = 3;
            const start = lv % step == 0 ? step : lv % step;
            for (let i = start; i <= lv; i += step) {
                await this.waitAI();
                if (id != this.drawId || !this.enableDrawEval) break;
                const status = await this.engine.getState(i)
                if (id != this.drawId || !this.enableDrawEval) break;
                this.ui.render(status, this.blackPlayerName, this.whitePlayerName);
                await new Promise(resolve => setTimeout(resolve, 0));
            }
        }
        else {
            this.drawId++;
            const id = this.drawId;
            await this.waitAI();
            if (id != this.drawId) return;
            const status = await this.engine.getState(null);
            if (id != this.drawId) return;
            this.ui.render(status, this.blackPlayerName, this.whitePlayerName);
        }
    }
    async endGame() {
        const status = await this.engine.getState(null);
        const blackCount = countBits(getBits(status, "black"));
        const whiteCount = countBits(getBits(status, "white"));
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
            await this.draw_force();
            if (await this.engine.isEnd()) this.endGame();
            this.isThinking = false;
            this.draw();
        });
    }
    async doOverClick() {
        if (this.isThinking) {
            console.log("AI is Thinking !");
            return;
        }
        this.isThinking = true;

        await this.undo();

        this.isThinking = false;
        this.draw();
    }

    async undo() {
        if (this.enableAi) {
            let max = 70;
            while (max--) {
                let before_undo_status = await this.engine.getState(null);
                const blackCount = countBits(getBits(before_undo_status, "black"));
                const whiteCount = countBits(getBits(before_undo_status, "white"));
                if (blackCount + whiteCount == 5 && this.aiTurn == "black") {
                    break;
                }

                await this.engine.undo();
                
                let after_undo_status = await this.engine.getState(null);
                if (after_undo_status.next_turn.toLowerCase() != this.aiTurn && !(await this.engine.isPass())) {
                    break;
                }
                await this.draw_force();
                await sleep(200);
            }
        } else {
            await this.engine.undo();
        }
    }
    async redoClick() {
        if (this.isThinking) {
            console.log("AI is Thinking !");
            return;
        }
        this.isThinking = true;

        await this.redo();

        this.isThinking = false;
        this.draw();
    }
    async redo() {
        await this.engine.redo();
    }

    async newGame() {
        while (this.isThinking) {
            // AIが応答するまで、何もない盤面を出し続ける
            await sleep(20);
            this.ui.render(undefined, this.blackPlayerName, this.whitePlayerName);
        }

        this.isThinking = true;
        await this.engine.newGame();
        await this.draw_force();
        if (this.aiTurn == "black" && this.enableAi){
            await sleep(600); 
            await this.aiPut();
        }
        this.isThinking = false;
        this.drawId = 0;
        this.draw();
    }

    async deepHintClick() {
        const depth = Number(window.prompt("現在の盤面の評価値をより正確に計算します。\n計算に使用するAIのレベル(1 ~ 24)を入力してください。\n(通常のHintボタンではレベル7の評価値を表示します。)\n(計算には、時間がかかることがあります。)"));
        if (!isNaN(depth) && 1 <= depth && depth <= 24) {
            this.isThinking = true;
            const init_status = await this.engine.getState(null);
            this.ui.render(init_status, this.blackPlayerName, this.whitePlayerName);
            const status = await this.engine.getState(depth);
            this.ui.render(status, this.blackPlayerName, this.whitePlayerName);
            this.drawId++;
            alert("計算が完了しました。");
            this.isThinking = false;
        } else {
            alert("無効な入力です。AIのレベル(1 ~ 24)を整数値で入力してください。");
        }
    }
    async switchEnableDrawEvalClick() {
        this.enableDrawEval = !this.enableDrawEval;
        this.draw();
    }

}
