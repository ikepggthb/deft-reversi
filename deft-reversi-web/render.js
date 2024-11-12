const X = 25;
const Y = 25;
const size = 540;
const padding = 25;
const cellMargin = 1.5;
const cellSize = (size - 25 * 2 - (cellMargin * (8 - 1))) / 8 ;
const cv = document.getElementById("cv");
const ctx = cv.getContext("2d");

const worker = new Worker('engine.js', { type: 'module' });


let isThinking = false;


worker.addEventListener('message', (message) => {
    if (message.data.type == "render") {
        render(message.data.status, worker);
    }
    if (message.data.type == "console_log") {
        console.log(message.data.data);
    }
    if (message.data.type == "alert") {
        alert(message.data.message)
    }
    if (message.data.type == "thinking") {
        isThinking = message.data.isThinking
    }
});

// 高解像度化
    // DPR とキャンバスの大きさを取得
    const dpr = window.devicePixelRatio;
    const rect = cv.getBoundingClientRect();

    // キャンバスの「実際の」大きさを設定
    cv.width = rect.width * dpr;
    cv.height = rect.height * dpr;

    // 正しい描画操作を保証するためにコンテキストの変倍
    ctx.scale(dpr, dpr);

    // キャンバスの「描画される」大きさを設定
    cv.style.width = `${rect.width}px`;
    cv.style.height = `${rect.height}px`;


function drawBackground() {
    const gradient = ctx.createLinearGradient(cv.width / 2, 0, cv.width / 2, cv.height);
    gradient.addColorStop(0, "#000000");
    gradient.addColorStop(1, "#345955");
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, cv.width, cv.height);

    ctx.fillStyle = "#000000";
    ctx.fillRect(X, Y, size, size);
}

function drawTiles() {
    for(let i = 0; i < 8; ++i){
        for(let j = 0; j < 8; ++j){
            ctx.fillStyle = "#009959";
            ctx.fillRect(
                X + padding + i * (cellSize + cellMargin),
                Y + padding + j * (cellSize + cellMargin),
                cellSize,
                cellSize
            );
        }
    }
}

function drawMarkers() {
    for(let i = 2; i <= 6; i += 4){
        for(let j = 2; j <= 6; j += 4){
            const x = X + padding + i * (cellSize + cellMargin) - cellMargin / 2;
            const y = Y + padding + j * (cellSize + cellMargin) - cellMargin / 2;
            const r = cellMargin * 2.5;

            ctx.beginPath();
            ctx.arc(x, y, r, 0, 2*Math.PI, false);
            ctx.fillStyle = "black";
            ctx.fill();
        }
    }
}

function clickEvent(event) {
    const rect = cv.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;

    const col = Math.floor((x - X - padding) / (cellSize + cellMargin));
    const row = Math.floor((y - Y - padding) / (cellSize + cellMargin));

    if (col >= 0 && col < 8 && row >= 0 && row < 8) {
        if (isThinking) {
            console.log("AI is Thinking !");
            return;
        }
        stopDrawMoveScores();
        isThinking = true;
        let post_item = {
            type: "put",
            position: row * 8 + col
        }
        worker.postMessage(post_item);
    } else {
        drawScore();
    }
}


function coordStoneX(x){
    return X + padding + x * (cellSize + cellMargin) + cellSize / 2;
}
function coordStoneY(y){
    return Y + padding + y * (cellSize + cellMargin) + cellSize / 2;
}

function drawStone(x, y, color) {
    const radius = cellSize / 2.5;

    ctx.shadowOffsetX = 1;
    ctx.shadowOffsetY = 1;
    ctx.shadowBlur = 8;
    ctx.shadowColor = "black";

    ctx.beginPath(); // 新しいパスを開始
    ctx.arc(coordStoneX(x), coordStoneY(y), radius, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
    ctx.closePath();

    
    ctx.shadowOffsetX = 0;
    ctx.shadowOffsetY = 0;
    ctx.shadowBlur    = 0;
}

function drawScore() {
    let post_item = {
        type: "draw_score"
    }
    worker.postMessage(post_item);
}

function drawMoveScores() {
    let post_item = {
        type: "draw_move_scores"
    }
    worker.postMessage(post_item);
}

function stopDrawMoveScores() {
    worker.postMessage({ type: "stop_draw_move_scores" });
}


export function render(status) {

    ctx.clearRect(0, 0, cv.width, cv.height);
    drawBackground();
    drawTiles();
    drawMarkers();

    cv.addEventListener('click', clickEvent);

    const black = status.black.split("").reverse().join("");
    const white = status.white.split("").reverse().join("");
    const legal_moves = status.legal_moves.split("").reverse().join("");


    let max_score = -64;
    if(status.eval) {
        for (let i = 0; i < 64; ++i){
            if (legal_moves.charAt(i) == "1") {
                const score = status.eval[i];
                if (max_score < score) {
                    max_score = score;
                }
            }
        }
    }
    
    for (let i = 0; i < 64; ++i) {
        
        const row = i % 8;
        const col = Math.floor(i / 8);
        if (black.charAt(i) == "1") {
            drawStone(row, col, "Black");
        }
        if (white.charAt(i) == "1") {
            drawStone(row, col, "White");
        }
        if (legal_moves.charAt(i) == "1") {
            ctx.fillStyle = "#60C969";
            ctx.fillRect(
                X + padding + row * (cellSize + cellMargin),
                Y + padding + col * (cellSize + cellMargin),
                cellSize,
                cellSize
            );
            if (status.eval) {
                const score = status.eval[i];
                if (max_score == score){
                    ctx.fillStyle = "#2077c0";
                } else {
                    ctx.fillStyle = "white";
                }
                ctx.font = "24px Arial";
                ctx.textAlign = "center";
                ctx.textBaseline = "middle";
                ctx.fillText(
                    score.toString(),
                    X + padding + row * (cellSize + cellMargin) + cellSize / 2,
                    Y + padding + col * (cellSize + cellMargin) + cellSize / 2
                );
            }
        }


    }

}
