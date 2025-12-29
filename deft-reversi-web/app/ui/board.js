/**
 * ゲームボードの視覚的な表現を管理します。
 * これには、盤面、石、座標、合法手、ヒントの描画が含まれます。
 */
export class BoardUI {
    constructor() {
        this.cv = document.getElementById("cv");
        this.ctx = this.cv.getContext("2d");

        const p1 = window.innerWidth / 600;
        const p2 = window.innerHeight / 800;
        const p = p1 < p2 ? p1 : p2;

        this.X = 25;
        this.Y = 25;
        this.size = 540;
        this.padding = 25;
        this.cellMargin = 1.5;
        this.cellSize = (this.size - 25 * 2 - this.cellMargin * (8 - 1)) / 8;
    }

    /**
     * ボードの背景を描画します。
     */
    drawBackground() {
        this.ctx.fillStyle = "#000000";
        this.ctx.fillRect(this.X, this.Y, this.size, this.size);
    }

    /**
     * 指定されたゲーム状態に基づいてボード全体を更新および再描画します。
     * @param {object} status 現在のゲーム状態。
     */
    update(status) {
        this.drawBackground();
        this.drawTiles();
        this.drawCoord();
        if (status) {
            this.drawStones(status);
            this.drawMoves(status);
            this.drawHumanOpeningNextPosition(status);
            this.drawScores(status);
            this.drawLastMoveMarker(status.lastMove);
        }
        this.drawMarkers();
    }

    /**
     * ボードのタイル（マス目）を描画します。
     */
    drawTiles() {
        for (let i = 0; i < 8; ++i) {
            for (let j = 0; j < 8; ++j) {
                this.ctx.fillStyle = "#009959";
                this.ctx.fillRect(
                    this.X + this.padding + i * (this.cellSize + this.cellMargin),
                    this.Y + this.padding + j * (this.cellSize + this.cellMargin),
                    this.cellSize,
                    this.cellSize,
                );
            }
        }
    }

    /**
     * ボード上の基準点マーカーを描画します。
     */
    drawMarkers() {
        for (let i = 2; i <= 6; i += 4) {
            for (let j = 2; j <= 6; j += 4) {
                const x = this.X + this.padding + i * (this.cellSize + this.cellMargin) - this.cellMargin / 2;
                const y = this.Y + this.padding + j * (this.cellSize + this.cellMargin) - this.cellMargin / 2;
                const r = this.cellMargin * 2.5;

                this.ctx.beginPath();
                this.ctx.arc(x, y, r, 0, 2 * Math.PI, false);
                this.ctx.fillStyle = "black";
                this.ctx.fill();
            }
        }
    }

    /**
     * ボードのx座標をキャンバスのx座標に変換します。
     * @param {number} x ボードの列インデックス (0-7)。
     * @returns {number} 対応するキャンバスのx座標。
     * @private
     */
    coordStoneX(x) {
        return this.X + this.padding + x * (this.cellSize + this.cellMargin) + this.cellSize / 2;
    }

    /**
     * ボードのy座標をキャンバスのy座標に変換します。
     * @param {number} y ボードの行インデックス (0-7)。
     * @returns {number} 対応するキャンバスのy座標。
     * @private
     */
    coordStoneY(y) {
        return this.Y + this.padding + y * (this.cellSize + this.cellMargin) + this.cellSize / 2;
    }

    /**
     * 指定された位置に単一の石を描画します。
     * @param {number} position 石を置く位置 (0-63)。
     * @param {string} color 石の色 ("Black" または "White")。
     */
    drawStone(position, color) {
        const x = position % 8;
        const y = Math.floor(position / 8);

        const radius = this.cellSize / 2.5;

        this.ctx.shadowOffsetX = 1;
        this.ctx.shadowOffsetY = 1;
        this.ctx.shadowBlur = 8;
        this.ctx.shadowColor = "black";

        this.ctx.beginPath(); // 新しいパスを開始
        this.ctx.arc(this.coordStoneX(x), this.coordStoneY(y), radius, 0, Math.PI * 2);
        this.ctx.fillStyle = color;
        this.ctx.fill();
        this.ctx.closePath();

        this.ctx.shadowOffsetX = 0;
        this.ctx.shadowOffsetY = 0;
        this.ctx.shadowBlur = 0;
    }

    /**
     * 現在のプレイヤーの合法手をハイライト表示します。
     * @param {object} status 現在のゲーム状態。
     */
    drawMoves(status) {
        const bits = status.legalMovesBits;
        if (bits == null) return;
        for (let i = 0; i < 64; ++i) {
            if (((bits >> BigInt(i)) & 1n) === 1n) {
                const row = i % 8;
                const col = Math.floor(i / 8);
                this.ctx.fillStyle = "#60C969";
                this.ctx.fillRect(
                    this.X + this.padding + row * (this.cellSize + this.cellMargin),
                    this.Y + this.padding + col * (this.cellSize + this.cellMargin),
                    this.cellSize,
                    this.cellSize,
                );
            }
        }
    }

    /**
     * 指定された位置に評価スコアを描画します。
     * @param {number} position スコアを描画する位置 (0-63)。
     * @param {number} score 描画するスコア。
     * @param {string} color スコアのテキスト色。
     * @private
     */
    drawScore(position, score, color) {
        const row = position % 8;
        const col = Math.floor(position / 8);
        this.ctx.fillStyle = color;
        this.ctx.font = "24px Arial";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";
        this.ctx.fillText(score.toString(), this.coordStoneX(row), this.coordStoneY(col));
    }

    /**
     * 定石の次の推奨手をハイライトします。
     * @param {object} status 現在のゲーム状態。
     * @private
     */
    drawHumanOpeningNextPosition(status) {
        if (status.humanOpeningNextPosition !== undefined && status.humanOpeningNextPosition !== null && status.eval) {
            const row = status.humanOpeningNextPosition % 8;
            const col = Math.floor(status.humanOpeningNextPosition / 8);
            this.ctx.fillStyle = "#F07050";
            this.ctx.fillRect(
                this.X + this.padding + row * (this.cellSize + this.cellMargin),
                this.Y + this.padding + col * (this.cellSize + this.cellMargin),
                this.cellSize,
                this.cellSize,
            );
        }
    }

    /**
     * 各合法手に対する評価スコア（ヒント）を描画します。
     * @param {object} status 現在のゲーム状態。
     */
    drawScores(status) {
        if (!status.eval) return;
        const bits = status.legalMovesBits;
        if (bits == null) return;

        let maxScore = null;
        for (let i = 0; i < 64; ++i) {
            if (((bits >> BigInt(i)) & 1n) === 1n) {
                const score = status.eval[i];
                if (score === null || score === undefined) continue;
                if (maxScore === null || score > maxScore) {
                    maxScore = score;
                }
            }
        }
        if (maxScore === null) return;

        for (let i = 0; i < 64; ++i) {
            if (((bits >> BigInt(i)) & 1n) === 1n) {
                const score = status.eval[i];
                if (score === null || score === undefined) continue;
                const color = maxScore == score ? "#2077c0" : "white";
                this.drawScore(i, score, color);
            }
        }
    }

    /**
     * 現在の盤面に基づいてすべての石を描画します。
     * @param {object} status 現在のゲーム状態。
     */
    drawStones(status) {
        const blackBits = status.blackBits;
        const whiteBits = status.whiteBits;
        if (blackBits == null || whiteBits == null) return;

        for (let i = 0; i < 64; ++i) {
            const mask = 1n << BigInt(i);
            if ((blackBits & mask) !== 0n) this.drawStone(i, "Black");
            if ((whiteBits & mask) !== 0n) this.drawStone(i, "White");
        }
    }

    /**
     * ボードの座標（A-H, 1-8）を描画します。
     */
    drawCoord() {
        const horizontal = "ABCDEFGH";
        const vertical = "12345678";

        this.ctx.fillStyle = "White";
        this.ctx.font = "16px Arial";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";

        const verticalY = this.Y + this.padding / 2;
        const horizontalX = this.X + this.padding / 2;

        for (let i = 0; i < 8; ++i) {
            this.ctx.fillText(horizontal[i], this.coordStoneX(i), verticalY);
            this.ctx.fillText(vertical[i], horizontalX, this.coordStoneY(i));
        }
    }

    /**
     * 最後の着手があった位置にマーカーを描画します。
     * @param {number | null} lastMove 最後の着手があった位置 (0-63)。
     */
    drawLastMoveMarker(lastMove) {
        if (lastMove === null || lastMove === undefined) return;

        const col = lastMove % 8; // 列
        const row = Math.floor(lastMove / 8); // 行

        const x = this.X + this.padding + col * (this.cellSize + this.cellMargin) + this.cellSize * 0.9; // 右下の位置
        const y = this.Y + this.padding + row * (this.cellSize + this.cellMargin) + this.cellSize * 0.9;

        const radius = 5; // 赤い点の半径

        this.ctx.beginPath();
        this.ctx.arc(x, y, radius, 0, Math.PI * 2);
        this.ctx.fillStyle = "#CB0000";
        this.ctx.fill();
        this.ctx.closePath();
    }

    /**
     * キャンバスの座標 (x, y) をボードの位置 (0-63) に変換します。
     * @param {number} x キャンバス上のx座標。
     * @param {number} y キャンバス上のy座標。
     * @returns {number | undefined} 対応するボードの位置。ボード外の場合はundefined。
     */
    getBoardPosition(x, y) {
        const col = Math.floor((x - this.X - this.padding) / (this.cellSize + this.cellMargin));
        const row = Math.floor((y - this.Y - this.padding) / (this.cellSize + this.cellMargin));
        if (col >= 0 && col < 8 && row >= 0 && row < 8) {
            return row * 8 + col;
        }
    }
}
