import { BoardUI } from "./board.js";
import { StatusUI } from "./status.js";
import { OPENINGS } from "./openings.js";

export class UI {
    constructor(eventDispatcher) {
        this.initModalWindow();
        this.initEndGameModalWindow();
        this.cv = document.getElementById("cv");
        this.ctx = this.cv.getContext("2d");
        this.initLogArea();

        this.scale = this.scale.bind(this);
        this.scale();
        window.addEventListener("resize", this.scale);

        this.board = new BoardUI();
        this.statusUI = new StatusUI();
        this.setButtons();

        this.eventDispatcher = eventDispatcher;
        this.cv.addEventListener("click", this.handleClick.bind(this));
        this.aiReady = false;
    }

    initLogArea() {
        this.logArea = document.createElement("div");
        this.logArea.id = "log-area";
        Object.assign(this.logArea.style, {
            position: "fixed",
            right: "16px",
            bottom: "16px",
            maxWidth: "320px",
            padding: "12px",
            borderRadius: "8px",
            background: "rgba(0,0,0,0.65)",
            color: "#fff",
            fontFamily: "Arial, sans-serif",
            fontSize: "14px",
            lineHeight: "1.4",
            zIndex: "999",
            display: "none",
        });
        document.body.appendChild(this.logArea);
        this.logTimeout = null;
    }

    logMessage(message, isError = false) {
        if (!this.logArea) return;
        this.logArea.textContent = message;
        this.logArea.style.display = "block";
        this.logArea.style.background = isError ? "rgba(176, 30, 30, 0.8)" : "rgba(0,0,0,0.65)";
        if (this.logTimeout) {
            clearTimeout(this.logTimeout);
        }
        this.logTimeout = setTimeout(() => {
            this.logArea.style.display = "none";
        }, 5000);
    }

    logError(message) {
        this.logMessage(message, true);
    }

    logInfo(message) {
        this.logMessage(message, false);
    }

    onAIReady() {
        const startButton = document.getElementById("start-button");
        startButton.textContent = "Game Start !";
        this.aiReady = true;
    }

    initModalWindow() {
        const aiToggle = document.getElementById("ai-toggle");
        const aiLevelSetting = document.getElementById("ai-level-setting");
        const turnSetting = document.getElementById("turn-setting");
        const modal = document.getElementById("modal");
        const startButton = document.getElementById("start-button");
        const aiLevelSlider = document.getElementById("ai-level-slider");
        const levelDisplay = document.getElementById("level-display");
        const openingSelect = document.getElementById("opening-strategy");

        this.setLevelDisplay = (lv) => {
            if (lv > 20) {
                levelDisplay.textContent = `Lv. ${aiLevelSlider.value} (非推奨)`;
            } else {
                levelDisplay.textContent = `Lv. ${aiLevelSlider.value}`;
            }
        };

        const firstButton = document.getElementById("first-button");
        const secondButton = document.getElementById("second-button");

        if (!this.aiReady) startButton.textContent = "Now Loading...";

        const savedSettingsString = localStorage.getItem("gameSettings");
        if (savedSettingsString !== null) {
            const savedSettings = JSON.parse(localStorage.getItem("gameSettings")) || {
                aiEnabled: true,
                aiLevel: 5,
                aiTurn: "white",
            };

            aiToggle.classList.toggle("active", savedSettings.aiEnabled);
            aiLevelSetting.classList.toggle("hidden", !savedSettings.aiEnabled);
            turnSetting.classList.toggle("hidden", !savedSettings.aiEnabled);
            aiLevelSlider.value = savedSettings.aiLevel;
            this.setLevelDisplay(savedSettings.aiLevel);

            if (savedSettings.aiTurn === "white") {
                firstButton.classList.add("active");
                secondButton.classList.remove("active");
            } else if (savedSettings.aiTurn === "black") {
                secondButton.classList.add("active");
                firstButton.classList.remove("active");
            }
        } else {
            modal.style.height = "auto";
            aiToggle.classList.toggle("active", true);
            aiLevelSetting.classList.toggle("hidden", false);
            turnSetting.classList.toggle("hidden", false);
            firstButton.classList.add("active");
            secondButton.classList.remove("active");
            aiLevelSlider.value = 1;
            this.setLevelDisplay(1);
        }

        aiLevelSlider.addEventListener("input", () => {
            this.setLevelDisplay(aiLevelSlider.value);
        });

        aiToggle.addEventListener("click", () => {
            aiToggle.classList.toggle("active");
            const isAiEnabled = aiToggle.classList.contains("active");

            aiLevelSetting.classList.toggle("hidden", !isAiEnabled);
            turnSetting.classList.toggle("hidden", !isAiEnabled);

            modal.style.height = "auto";
        });

        firstButton.addEventListener("click", () => {
            firstButton.classList.add("active");
            secondButton.classList.remove("active");
        });
        secondButton.addEventListener("click", () => {
            secondButton.classList.add("active");
            firstButton.classList.remove("active");
        });

        startButton.addEventListener(
            "click",
            (() => {
                if (!this.aiReady) return;
                const aiTurn = (() => {
                    if (firstButton.classList.contains("active")) {
                        return "white";
                    } else if (secondButton.classList.contains("active")) {
                        return "black";
                    }
                })();

                const settings = {
                    aiEnabled: aiToggle.classList.contains("active"),
                    aiLevel: parseInt(aiLevelSlider.value),
                    aiTurn: aiTurn,
                    humanOpening: openingSelect.value,
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

                localStorage.setItem("gameSettings", JSON.stringify(settings));
                this.eventDispatcher.dispatchEvent("setEnableAI", settings.aiEnabled);
                this.eventDispatcher.dispatchEvent("setAILevel", settings.aiLevel);
                this.eventDispatcher.dispatchEvent("setAITurn", settings.aiTurn);
                this.eventDispatcher.dispatchEvent(
                    "setPlayerName",
                    this.blackPlayerName,
                    this.whitePlayerName,
                );
                this.eventDispatcher.dispatchEvent("setHumanOpening", settings.humanOpening);
                this.eventDispatcher.dispatchEvent("newGameClick");

                const modalOverlay = document.getElementById("modal-overlay");
                modalOverlay.classList.add("fade-out");
                modalOverlay.addEventListener(
                    "animationend",
                    () => {
                        modalOverlay.style.display = "none";
                    },
                    { once: true },
                );
            }).bind(this),
        );

        this.addOpenings();
    }

    addOpenings() {
        const openingSelect = document.getElementById("opening-strategy");
        for (const opening of OPENINGS) {
            const [index, name] = opening;
            const option = document.createElement("option");
            option.value = index;
            option.text = name;
            openingSelect.appendChild(option);
        }
    }

    initEndGameModalWindow() {
        this.endGameModalOverlay = document.getElementById("end-game-modal-overlay");
        this.gameHistory = document.getElementById("game-history");
        this.copyHistoryButton = document.getElementById("copy-history-button");
        this.closeEndGameModal = document.getElementById("close-end-game-modal");

        this.copyHistoryButton.addEventListener("click", () => {
            navigator.clipboard.writeText(this.gameHistory.value).then(() => {
                alert("棋譜がコピーされました！");
            });
        });

        this.closeEndGameModal.addEventListener("click", () => {
            this.endGameModalOverlay.style.display = "none";
        });
    }

    showEndGameModal(blackScore, whiteScore, blackPlayerName, whitePlayerName, history) {
        const endGameModalOverlay = document.getElementById("end-game-modal-overlay");
        const blackScoreElement = document.getElementById("black-score");
        const whiteScoreElement = document.getElementById("white-score");
        const winnerText = document.getElementById("winner-text");
        const gameHistory = document.getElementById("game-history");

        blackScoreElement.textContent = blackScore;
        whiteScoreElement.textContent = whiteScore;

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

        gameHistory.value = history;
        endGameModalOverlay.style.display = "flex";
    }

    hideEndGameModal() {
        this.endGameModalOverlay.style.display = "none";
    }

    showModalWindow() {
        const modalOverlay = document.getElementById("modal-overlay");
        modalOverlay.classList.remove("fade-out");
        modalOverlay.style.display = "";
    }

    scale() {
        const scale = Math.min(window.innerWidth / this.cv.width, window.innerHeight / this.cv.height);
        this.cv.style.width = `${this.cv.width * scale}px`;
        this.cv.style.height = `${this.cv.height * scale}px`;
    }

    drawPassMessage() {
        const centerX = this.cv.width / 2;
        const centerY = this.cv.height / 2;

        this.ctx.fillStyle = "rgba(0, 0, 0, 0.8)";
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
                onClick: (() => {
                    this.showModalWindow();
                }).bind(this),
            },
            {
                label: "Undo",
                onClick: (() => {
                    this.eventDispatcher.dispatchEvent("doOverClick");
                }).bind(this),
            },
            {
                label: "Redo",
                onClick: (() => {
                    this.eventDispatcher.dispatchEvent("redoClick");
                }).bind(this),
            },
            {
                label: "Hint",
                onClick: (() => {
                    this.eventDispatcher.dispatchEvent("switchShowEvalClick");
                }).bind(this),
            },
            {
                label: "Hint\n(Deep)",
                onClick: (() => {
                    this.eventDispatcher.dispatchEvent("deepHintClick");
                }).bind(this),
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

        const position = this.board.getBoardPosition(x, y);
        if (position !== undefined) {
            this.eventDispatcher.dispatchEvent("boardClick", position);
            return;
        }

        this.statusUI.clickButton(x, y);
    }

    render(status, blackPlayerName, whitePlayerName) {
        this.ctx.clearRect(0, 0, this.cv.width, this.cv.height);
        this.board.update(status);
        this.statusUI.update(status, blackPlayerName, whitePlayerName);
    }
}
