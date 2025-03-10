const OPENINGS = [
    [0, "縦取り"],
    [1, "虎系"],
    [2, "虎定石"],
    [3, "BergTiger"],
    [4, "イエス流"],
    [5, "Aubrey(Feldborg)"],
    [6, "イエス流(クラシック)"],
    [7, "ブライトウェル"],
    [8, "ブライトウェル(Leader)"],
    [9, "蝶定石"],
    [10, "イエス流(edmead)"],
    [11, "イエス流(引っ張り進行)"],
    [12, "ブライトウェル・ウィング"],
    [13, "シャチ"],
    [14, "リーダーズタイガー"],
    [15, "ビン定石"],
    [16, "ビン定石・坂口流"],
    [17, "リーダーズタイガー(三角形)"],
    [18, "リーダーズタイガー(飛び出し型)"],
    [19, "リーダーズタイガー(斜め飛び出し型)"],
    [20, "榎本トラップ"],
    [21, "Stephenson"],
    [22, "コンポス"],
    [23, "砂虎"],
    [24, "砂虎(Ralle)"],
    [25, "ライトニングボルト"],
    [26, "ライトニング返し"],
    [27, "たまプラ～ザ"],
    [28, "最強定石"],
    [29, "一刀両断"],
    [30, "ロジステロ流"],
    [31, "ろじくすぴあ"],
    [32, "satobonクロス"],
    [33, "テバポス"],
    [34, "D8コンポス"],
    [35, "プチポス"],
    [36, "シャープコンポス"],
    [37, "フラットコンポス"],
    [38, "f8コンポス"],
    [39, "H6コンポス"],
    [40, "スリポス"],
    [41, "ジャンポス"],
    [42, "F.A.T.Draw"],
    [43, "E2コンポス"],
    [44, "ノーカン"],
    [45, "ブライトウェルもどき"],
    [46, "ノーカン(Continuation)"],
    [47, "1993年新手"],
    [48, "超斬新なノーカン"],
    [49, "縦取り殺され"],
    [50, "Holgersson/家康流"],
    [51, "EJ"],
    [52, "ノーカン・11e7"],
    [53, "北島流"],
    [54, "ボンド定石"],
    [55, "金田流"],
    [56, "ノーカン(橋本流)"],
    [57, "カン/イタリアンクロス"],
    [58, "斜めカン"],
    [59, "斜めカン(e2)"],
    [60, "斜めカン(f3)"],
    [61, "カン(クラシック)"],
    [62, "虎大量"],
    [63, "バナナ定石"],
    [64, "南十字"],
    [65, "Ishii"],
    [66, "虎メインライン"],
    [67, "バナナ定石(レクイエム)"],
    [68, "バナナ定石(金田流)"],
    [69, "まこ虎"],
    [70, "あっくん定石/トパーズ"],
    [71, "Noローズビル"],
    [72, "花形定石ローズビル"],
    [73, "FJT定石"],
    [74, "みこし直前分岐"],
    [75, "クリスマスツリー"],
    [76, "幣(ぬさ)"],
    [77, "パブリックドロー"],
    [78, "FJT(クラシック)"],
    [79, "金魚定石"],
    [80, "りと虎"],
    [81, "すぱりと"],
    [82, "ためてん"],
    [83, "大箱金魚"],
    [84, "小箱金魚"],
    [85, "d7金魚"],
    [86, "b6金魚"],
    [87, "ソフィア"],
    [88, "パズル"],
    [89, "酉定石"],
    [90, "酉フック"],
    [91, "結論酉"],
    [92, "酉アッパー"],
    [93, "酉ストレート"],
    [94, "酉キック/酉ジャブ"],
    [95, "酉エッグ"],
    [96, "くらげ/斜め酉"],
    [97, "横酉"],
    [98, "辺酉"],
    [99, "へろ虎/ヒーロー"],
    [100, "虎スマイル"],
    [101, "F6ローズビル/三郎"],
    [102, "X虎大量"],
    [103, "龍定石"],
    [104, "風神定石"],
    [105, "竜巻定石"],
    [106, "水神定石"],
    [107, "天狗定石"],
    [108, "龍全滅I"],
    [109, "龍全滅II"],
    [110, "龍全滅III"],
    [111, "ミサイル定石/ギャング"],
    [112, "ザリガニ定石"],
    [113, "ザリガニ全滅"],
    [114, "ザリガニ外し"],
    [115, "海老定石"],
    [116, "虎系犬素"],
    [117, "猫系"],
    [118, "猫定石"],
    [119, "猫定石・坂口流"],
    [120, "暴走猫"],
    [121, "バル猫/裏tanida"],
    [122, "猫快速"],
    [123, "Berner"],
    [124, "ネオキャット"],
    [125, "歌猫"],
    [126, "ブラックヒル(黒山)"],
    [127, "nicolet"],
    [128, "猫全滅"],
    [129, "NoCat"],
    [130, "燕定石"],
    [131, "NoCat(Continuation)"],
    [132, "クリオネ"],
    [133, "ファルコン"],
    [134, "カシオペア"],
    [135, "ラッコ定石"],
    [136, "10手全滅"],
    [137, "羊定石"],
    [138, "羊白髪"],
    [139, "雷定石/ラバーズ・リープ"],
    [140, "雷定石大量取り"],
    [141, "猫系カバ素"],
    [142, "兎系"],
    [143, "兎外し"],
    [144, "兎外し・逆"],
    [145, "兎定石"],
    [146, "大和久流"],
    [147, "邪気流"],
    [148, "竹田流"],
    [149, "三村流"],
    [150, "玉家流"],
    [151, "クロス兎"],
    [152, "井上流"],
    [153, "雪うさぎ"],
    [154, "真美流"],
    [155, "穴兎"],
    [156, "井上流外し"],
    [157, "Bhagat"],
    [158, "Shaman/Danish/EO兎"],
    [159, "横うさぎ"],
    [160, "天魔の横うさぎ"],
    [161, "たまうさぎ"],
    [162, "Ralle定石"],
    [163, "Ralle定石・中島流"],
    [164, "ローズ"],
    [165, "Sローズ"],
    [166, "Sローズ・基本形"],
    [167, "Sローズ為則ローズ"],
    [168, "パラソル"],
    [169, "Sローズローテーション型(g5)"],
    [170, "Brightstein"],
    [171, "Sローズローテーション型(g6)"],
    [172, "Sローズ・13e7兜割り型"],
    [173, "Fローズ"],
    [174, "ブービーローズ"],
    [175, "Fローズローテーション型"],
    [176, "村上流"],
    [177, "横フラットローズ"],
    [178, "Fローズ兜割り"],
    [179, "Fローズローテーション型(Kling)"],
    [180, "新フラットローズ"],
    [181, "うえのん定石"],
    [182, "三本目ローズ"],
    [183, "上野ロズスペ"],
    [184, "X打ちローズ(Draw)"],
    [185, "ロズスペ"],
    [186, "ジーローズ"],
    [187, "地獄フラットローズ"],
    [188, "蜘蛛の糸"],
    [189, "手塚システム"],
    [190, "逆ローズ"],
    [191, "ruru兎"],
    [192, "馬定石"],
    [193, "馬全滅"],
    [194, "ユニコーン"],
    [195, "鹿定石"],
    [196, "鶴定石/梅沢流"],
    [197, "最短全滅"],
    [198, "野兎定石"],
    [199, "地獄兎"],
    [200, "地獄兎全滅I"],
    [201, "地獄兎全滅II"],
    [202, "斜め取り"],
    [203, "WingVariation"],
    [204, "SemiWingVariation"],
    [205, "牛定石"],
    [206, "Rose-v-Toth"],
    [207, "Tanida"],
    [208, "Aircraft/Feldborg"],
    [209, "谷口流"],
    [210, "快速船"],
    [211, "快速船定石大量取り"],
    [212, "幽霊船"],
    [213, "ヨット定石"],
    [214, "沈没船"],
    [215, "逆沈没船"],
    [216, "カヌー定石"],
    [217, "戦車定石"],
    [218, "戦車ローズ"],
    [219, "シャープ戦車"],
    [220, "シャープ戦車・16f2変化"],
    [221, "フラット戦車"],
    [222, "戦車システム"],
    [223, "椅子定石"],
    [224, "Landau"],
    [225, "Maruoka"],
    [226, "白大量取り/リンゴ定石"],
    [227, "ホットドッグ"],
    [228, "ピストル"],
    [229, "ヨーグルトプリン"],
    [230, "暴走ヨーグルトプリン"],
    [231, "らいとんバズーカ"],
    [232, "こうもり定石"],
    [233, "こうもり定石(KlingAlternative)"],
    [234, "Melnikov/Bat(Piau1)"],
    [235, "こうもり定石(Piau2)"],
    [236, "こうもり定石(KlingContinuation)"],
    [237, "潜水艦"],
    [238, "エヴァッカニア/うえのん定石part2"],
    [239, "石橋流"],
    [240, "飛び出し牛/闘牛"],
    [241, "白裏大量"],
    [242, "白裏スマッシュ"],
    [243, "裏石橋流"],
    [244, "裏石橋流全滅I"],
    [245, "裏石橋流全滅II"],
    [246, "裏ヨーグルトプリン"],
    [247, "野いちご"],
    [248, "裏飛行機"],
    [249, "裏ヨット"],
    [250, "飛び出し村上流"],
    [251, "岩崎流"],
    [252, "裏こうもり"],
    [253, "暴走裏こうもり"],
    [254, "足蟹"],
    [255, "挟み蟹"],
    [256, "ハネ蟹"],
    [257, "裏こうもり・11手目f7"],
    [258, "オクトパス"],
    [259, "三村流II"],
    [260, "ヘビ定石"],
    [261, "つちのこ"],
    [262, "ピンボール定石"],
    [263, "コブラ定石"],
    [264, "砂ヘビ"],
    [265, "マムシ"],
    [266, "バシリスク定石大量取り"],
    [267, "蛇全滅"],
    [268, "しもへび"],
    [269, "バッファロー定石/猛牛"],
    [270, "北陸バッファロー"],
    [271, "谷田バッファロー"],
    [272, "手塚システムpart1"],
    [273, "丸岡バッファロー"],
    [274, "たぬき"],
    [275, "たぬき狩り"],
    [276, "裏狸定石"],
    [277, "もしもピアノに轢かれたら"],
    [278, "裏蛇定石"],
    [279, "ロケット定石/ヒラメ定石"],
    [280, "Hamilton"],
    [281, "Lollipop"],
    [282, "暴走牛"],
    [283, "暴走牛全滅"],
    [284, "平行取り"],
    [285, "チューペット"],
    [286, "裏チューペット"],
    [287, "ヨット鼠"],
    [288, "旅チュー"],
    [289, "旅チュー・逆"],
    [290, "とん鼠"],
    [291, "鼠定石"],
    [292, "鼠全滅"],
    [293, "鼠全滅II"],
    [294, "裏鼠定石"],
];


import { BoardUI } from "./board.js";
import { StatusUI } from "./status.js";

const sleep = (time) => new Promise((resolve) => setTimeout(resolve, time));//timeはミリ秒

function count(str){
    let c = 0;    
    for (var i = 0; i < str.length; i++) {
        if (str[i] == "1") c++;
    }
    return c;
};


class Modal {
    constructor() {
        this.aiToggle = document.getElementById('ai-toggle');
        this.aiLevelSetting = document.getElementById('ai-level-setting');
        this.turnSetting = document.getElementById('turn-setting');
        this.modal = document.getElementById('modal');
        this.startButton = document.getElementById('start-button');
        this.aiLevelSlider = document.getElementById('ai-level-slider');
        this.levelDisplay = document.getElementById('level-display');
        this.openingSetting = document.getElementById('opening-setting');
        this.openingSelect = document.getElementById('opening-strategy');
    }   
}

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
        this.setButtons();

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
        const openingSetting = document.getElementById('opening-setting');
        const openingSelect = document.getElementById('opening-strategy');

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

            modal.style.height = 'auto';
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
                humanOpening: openingSelect.value
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
            this.eventDispatcher.dispatchEvent('setHumanOpening', settings.humanOpening )
            this.eventDispatcher.dispatchEvent('newGameClick');

            // モーダルをフェードアウト
            const modalOverlay = document.getElementById('modal-overlay');
            modalOverlay.classList.add('fade-out');

            // フェードアウトが完了した後に非表示にする
            modalOverlay.addEventListener('animationend', () => {
                modalOverlay.style.display = 'none';
            }, { once: true });
        }).bind(this));

        this.addOpenings();
    }

    addOpenings() {
        const openingSelect = document.getElementById('opening-strategy');
        const openings = OPENINGS;

        for (const opening of openings) {
            const [index, name] = opening;
            let op = document.createElement('option');
            op.value = index;
            op.text = name;       
            openingSelect.appendChild(op);
        }
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

    setButtons() {
        const buttons = [
            {
                label: "New Game",
                onClick: (() => { this.showModalWindow();}).bind(this)
            },
            {
                label: "Undo",
                onClick: (() => { this.eventDispatcher.dispatchEvent('doOverClick'); }).bind(this)
            },
            {
                label: "Redo",
                onClick: (() => { this.eventDispatcher.dispatchEvent('redoClick'); }).bind(this)
            },
            {
                label: "Hint",
                onClick: (() => { this.eventDispatcher.dispatchEvent('switchShowEvalClick'); }).bind(this)
            },
            {
                label: "Hint\n(Deep)",
                onClick: (() => { this.eventDispatcher.dispatchEvent('deepHintClick'); }).bind(this)
            },
        ];

        this.statusUI.setButtons(buttons);
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

        // buttonClick
        this.statusUI.clickButton(x, y);
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
            'setHumanOpening'
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
                await this.draw_force();
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
                    await this.engine.setHumanOpening(f);
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
                const blackCount = count(before_undo_status.black);
                const whiteCount = count(before_undo_status.white);
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