
class Button {
    constructor() {
        this.cv = document.getElementById("cv");
        this.ctx = cv.getContext("2d");

        this.X = 25;
        this.Y = 25;
        this.size = 540;
        this.padding = 25;
        this.cellMargin = 1.5;
        this.cellSize = (this.size - 25 * 2 - (this.cellMargin * (8 - 1))) / 8;
    }
    draw_button() {
        ctx.fillStyle = "#FFFFFF";
        ctx.fillRect(
            X + padding,
            Y + size + padding * 2 + padding,
            100,
            30
        );
        ctx.fillStyle = "#000000";
        ctx.font = "24px Arial";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(
            "undo".toString(),
            X + padding + 100 / 2,
            Y + size + padding * 2 + padding + 30 / 2,
        );
    }
    isClick(x, y) {
        return X + padding < x && x < X + padding + 100 && Y + size + padding * 2 + padding < y && y < Y + size + padding * 2 + padding + 30
    }
}

class Buttons{
    
}