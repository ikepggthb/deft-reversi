export class Board
{
    constructor() {
        this.cv = document.getElementById("cv");
        this.ctx = cv.getContext("2d");

        this.X = 25;
        this.Y = 25;
        this.size = 540;
        this.padding = 25;
        this.cellMargin = 1.5;
        this.cellSize = (this.size - 25 * 2 - (this.cellMargin * (8 - 1))) / 8 ;

        this.draw();
    }

    drawBackground() {
        const gradient = this.ctx.createLinearGradient(this.cv.width / 2, 0, this.cv.width / 2, this.cv.height);
        gradient.addColorStop(0, "#000000");
        gradient.addColorStop(1, "#345955");
        this.ctx.fillStyle = gradient;
        this.ctx.fillRect(0, 0, this.cv.width, this.cv.height);

        this.ctx.fillStyle = "#000000";
        this.ctx.fillRect(this.X, this.Y, this.size, this.size);
    }

    drawTiles() {
        for(let i = 0; i < 8; ++i){
            for(let j = 0; j < 8; ++j){
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
        for(let i = 2; i <= 6; i += 4){
            for(let j = 2; j <= 6; j += 4){
                const x = this.X + this.padding + i * (this.cellSize + this.cellMargin) - this.cellMargin / 2;
                const y = this.Y + this.padding + j * (this.cellSize + this.cellMargin) - this.cellMargin / 2;
                const r = this.cellMargin * 2.5;

                this.ctx.beginPath();
                this.ctx.arc(x, y, r, 0, 2*Math.PI, false);
                this.ctx.fillStyle = "black";
                this.ctx.fill();
            }
        }
    }

    draw() {
        this.drawBackground();
        this.drawTiles();
        this.drawMarkers();

        this.cv.addEventListener('click', function(event) {
            const rect = this.cv.getBoundingClientRect();
            const x = event.clientX - rect.left;
            const y = event.clientY - rect.top;

            const col = Math.floor((x - this.X - this.padding) / (this.cellSize + this.cellMargin));
            const row = Math.floor((y - this.Y - this.padding) / (this.cellSize + this.cellMargin));

            if (col >= 0 && col < 8 && row >= 0 && row < 8) {
                // console.log(`Clicked on cell: row=${row}, col=${col}`);
                const s = new Stone(col, row, "Black", this.cv, this.ctx, this); // 例としてクリックした場所に黒石を置く
            }
        }.bind(this));

    }

}


class Stone {
    constructor(x, y, color, cv, ctx, boardObject) {
        this.cv = cv;
        this.ctx = ctx;
        this.board = boardObject;
        this.color = color;
        this.draw(x, y, color);
    }
    stoneX(x){
        return this.board.X + this.board.padding + x * (this.board.cellSize + this.board.cellMargin) + this.board.cellSize / 2;
    }
    stoneY(y){
        return this.board.Y + this.board.padding + y * (this.board.cellSize + this.board.cellMargin) + this.board.cellSize / 2;
    }

    draw(x, y, color) {
        const radius = this.board.cellSize / 2.5;

        this.ctx.shadowOffsetX = 1;
        this.ctx.shadowOffsetY = 1;
        this.ctx.shadowBlur = 8;
        this.ctx.shadowColor = "black";

        this.ctx.beginPath(); // 新しいパスを開始
        this.ctx.arc(this.stoneX(x), this.stoneY(y), radius, 0, Math.PI * 2);
        this.ctx.fillStyle = color;
        this.ctx.fill();
        this.ctx.closePath();
    }
}
