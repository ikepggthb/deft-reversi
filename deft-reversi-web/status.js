export class StatusUI {
    constructor() {
        this.cv = document.getElementById("cv");
        this.ctx = this.cv.getContext("2d");

        this.X = 25;
        this.Y = 570;
        this.W = 540;
        this.H = 180;
        this.padding = 25; 
        this.dividSpace = 10;

        this.topSectionHeight = ((this.H - this.padding * 2) / 2 - this.dividSpace);

        this.drawBackground();
        this.drawStatusArea();
        this.drawButtons();
        this.setButtons();
    }


    drawBackground() {
        // 黒い背景
        this.ctx.fillStyle = "#000000";
        this.ctx.fillRect(this.X, this.Y, this.W, this.H);
    }

    drawStatusArea() {
        // 試合状況を表示する場所
        this.ctx.fillStyle = "#D3D3D3";
        this.ctx.fillRect(
            this.X + this.padding,
            this.Y + this.padding,
            this.W - this.padding * 2,
            this.topSectionHeight
        );

        // ボタンを配置する場所
        this.ctx.fillStyle = "#D3D3D3";
        this.ctx.fillRect(
            this.X + this.padding,
            this.Y + this.padding + this.topSectionHeight + this.dividSpace,
            this.W - this.padding * 2,
            this.topSectionHeight
        );
    }

    setButtons() {
        this.buttons = [
            {
                label: "New Game",
            },
            {
                label: "Undo"
            },
            {
                label: "Redo"
            },
            {
                label: "Hint"
            },
            {
                label: "Hint\n(Deep)"
            },
            // {
            //     name: "open setting window",
            //     clickEvent: null,
            //     label: "設定"
            // }
        ];

        // ボタンの描画
        this.buttonAreaX = this.X + this.padding;
        this.buttonAreaY = this.Y + this.padding + this.topSectionHeight + this.dividSpace;
        this.buttonAreaW = this.W - this.padding * 2;
        this.buttonAreaH = this.topSectionHeight;

        this.buttonPadding = 10;
        this.buttonCount = this.buttons.length;
        this.buttonWidth = (this.buttonAreaW - (this.buttonCount + 1) * this.buttonPadding) / this.buttonCount;
        this.buttonHeight = this.buttonAreaH - this.buttonPadding * 2;
    }

    drawButtons() {
        for (let i = 0; i < this.buttonCount; i++) {
            const buttonX = this.buttonAreaX + this.buttonPadding + (this.buttonWidth + this.buttonPadding) * i;
            const buttonY = this.buttonAreaY + this.buttonPadding;
            this.drawButton(buttonX, buttonY, this.buttonWidth, this.buttonHeight, this.buttons[i].label);
        }
    }

    isClickButton(x, y) {
        for (let i = 0; i < this.buttonCount; i++) {
            const buttonX = this.buttonAreaX + this.buttonPadding + (this.buttonWidth + this.buttonPadding) * i;
            const buttonY = this.buttonAreaY + this.buttonPadding;

            const inX = buttonX <= x && x <= buttonX + this.buttonWidth;
            const inY = buttonY <= y && y <= buttonY + this.buttonHeight;
            if(inX && inY){
                return i;
            }
        }
        return null;
    }

    drawButton(x, y, width, height, label) {

        // ボタンの背景
        this.ctx.fillStyle = "#000000";
        this.ctx.fillRect(x, y, width, height);

        // ボタンの枠線
        this.ctx.strokeStyle = "#5a5a5a";
        this.ctx.strokeRect(x, y, width, height);

        // ボタンのラベル
        this.ctx.font = "16px Arial";
        this.ctx.fillStyle = "#FFFFFF";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";
        this.ctx.fillText(label, x + width / 2, y + height / 2);
    }

    drawStatus(status, blackPlayerName, whitePlayerName) {
        if(!status) return;
        let count = (str) => {
            let c = 0;
            for (var i = 0; i < str.length; i++) {
                if (str[i] == "1") c++;
            }
            return c;
        };
        const blackCount = count(status.black);
        const whiteCount = count(status.white);

        this.ctx.font = "16px Arial";
        this.ctx.fillStyle = "#000000";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";

        // 上部エリアの中央に配置
        const statusY = this.Y + this.padding + this.topSectionHeight / 2;

        this.ctx.fillStyle = "black";

        this.ctx.fillText(blackPlayerName, this.X + this.padding + (this.W - this.padding * 2) * 0.1, statusY);

        // 黒石のアイコン
        this.drawStoneIcon(this.X + this.padding + (this.W - this.padding * 2) * 0.25, statusY, "Black");

        // 黒石の数
        this.ctx.fillText(blackCount, this.X + this.padding + (this.W - this.padding * 2) * 0.325, statusY);


        // 白石の数
        this.ctx.fillStyle = "black";
        this.ctx.fillText(whiteCount, this.X + this.padding + (this.W - this.padding * 2) * 0.675, statusY);

        // 白石のアイコン
        this.drawStoneIcon(this.X + this.padding + (this.W - this.padding * 2) * 0.75, statusY, "White");

        // 「AI Lv16」のテキスト
        this.ctx.fillStyle = "black";
        this.ctx.fillText(whitePlayerName, this.X + this.padding + (this.W - this.padding * 2) * 0.9, statusY);

        this.ctx.font = "14px san-serif";
        this.ctx.fillStyle = "#000000";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "middle";
        if (status.current_human_opening) {
            this.ctx.fillText(status.current_human_opening, this.X + this.padding + (this.W - this.padding * 2) * 0.5, statusY);
        }
    }

    drawStoneIcon(x, y, color) {
        const radius = 15;

        this.ctx.shadowOffsetX = 1;
        this.ctx.shadowOffsetY = 1;
        this.ctx.shadowBlur = 8;
        this.ctx.shadowColor = "black";

        this.ctx.beginPath();
        this.ctx.arc(x, y, radius, 0, Math.PI * 2);
        this.ctx.fillStyle = color;
        this.ctx.fill();
        this.ctx.closePath();

        this.ctx.shadowOffsetX = 0;
        this.ctx.shadowOffsetY = 0;
        this.ctx.shadowBlur = 0;
    }

    update(status, blackPlayerName, whitePlayerName) {
        // ステータスエリアを再描画
        this.ctx.clearRect(this.X, this.Y, this.W, this.H);
        this.drawBackground();
        this.drawStatusArea();
        this.drawButtons();
        this.drawStatus(status, blackPlayerName, whitePlayerName); // AIのレベルを固定で16に設定
    }
}
