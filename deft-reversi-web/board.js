export class BoardUI {
    constructor() {
        this.cv = document.getElementById("cv");
        this.ctx = cv.getContext("2d");
        
        const p1 = window.innerWidth / 600;
        const p2 = window.innerHeight / 800;
        const p = p1 < p2 ? p1 : p2;

        this.X = 25;
        this.Y = 25;
        this.size = 540;
        this.padding = 25;
        this.cellMargin = 1.5;
        this.cellSize = ((this.size - 25 * 2 - (this.cellMargin * (8 - 1))) / 8);
    }

    drawBackground() {
        this.ctx.fillStyle = "#000000";
        this.ctx.fillRect(this.X, this.Y, this.size, this.size);
    }

    update(status) {
        this.drawBackground();
        this.drawTiles();
        this.drawCoord();
        if (status) {
            this.drawStones(status);
            this.drawMoves(status);
            this.drawScores(status);
            this.drawLastMoveMarker(status.last_move);
        }
        this.drawMarkers();
    }

    drawTiles() {
        for (let i = 0; i < 8; ++i) {
            for (let j = 0; j < 8; ++j) {
                this.ctx.fillStyle = "#009959";
                this.ctx.fillRect(
                    this.X + this.padding + i * (this.cellSize + this.cellMargin),
                    this.Y + this.padding + j * (this.cellSize + this.cellMargin),
                    this.cellSize,
                    this.cellSize
                );
            }
        }
    }

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
    
    coordStoneX(x) {
        return this.X + this.padding + x * (this.cellSize + this.cellMargin) + this.cellSize / 2;
    }

    coordStoneY(y) {
        return this.Y + this.padding + y * (this.cellSize + this.cellMargin) + this.cellSize / 2;
    }

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

    drawMoves(status) {
        if (!status.legal_moves) return;
        const legal_moves = status.legal_moves.split("").reverse().join("");
        for (let i = 0; i < 64; ++i) {
            const row = i % 8;
            const col = Math.floor(i / 8);
            if (legal_moves.charAt(i) == "1") {
                this.ctx.fillStyle = "#60C969";
                this.ctx.fillRect(
                    this.X + this.padding + row * (this.cellSize + this.cellMargin),
                    this.Y + this.padding + col * (this.cellSize + this.cellMargin),
                    this.cellSize,
                    this.cellSize
                );
            }
        }
    }

    drawScore(position, score, color) {
        const row = position % 8;
        const col = Math.floor(position / 8);
        this.ctx.fillStyle = color;
        this.ctx.font = "24px Arial";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";
        this.ctx.fillText(
            score.toString(),
            this.coordStoneX(row),
            this.coordStoneY(col)
        );
    }

    drawScores(status) {
        if (!status.eval || !status.legal_moves) return;

        const legal_moves = status.legal_moves.split("").reverse().join("");

        let max_score = -64;
        for (let i = 0; i < 64; ++i) {
            if (legal_moves.charAt(i) == "1") {
                if (status.eval[i] > max_score){
                    max_score = status.eval[i];
                }
            }
        }
        for (let i = 0; i < 64; ++i) {
            if (legal_moves.charAt(i) == "1") {
                const score = status.eval[i];
                const color = max_score == score ? "#2077c0" : "white";
                this.drawScore(i, score, color);
            }

        }
    }

    drawStones(status) {
        if (!status.black || !status.white) return;
        const black = status.black.split("").reverse().join("");
        const white = status.white.split("").reverse().join("");

        for (let i = 0; i < 64; ++i) {
            if (black.charAt(i) == "1") {
                this.drawStone(i, "Black");
            }
            if (white.charAt(i) == "1") {
                this.drawStone(i, "White");
            }

        }
    }

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
            this.ctx.fillText(
                horizontal[i],
                this.coordStoneX(i),
                verticalY
            );
            this.ctx.fillText(
                vertical[i],
                horizontalX,
                this.coordStoneY(i)
            );
        }
    }


    drawLastMoveMarker(lastMove) {
        if (!lastMove) return;

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

    getBoardPosition(x, y) {
        const col = Math.floor((x - this.X - this.padding) / (this.cellSize + this.cellMargin));
        const row = Math.floor((y - this.Y - this.padding) / (this.cellSize + this.cellMargin));
        if (col >= 0 && col < 8 && row >= 0 && row < 8) {
            return row * 8 + col;
        } 
    }

}