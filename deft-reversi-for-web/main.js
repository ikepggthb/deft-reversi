import __wbg_init, { add, App } from "./pkg/deft_web.js";

async function decompressGzip(compressedData) {
    const ds = new DecompressionStream('gzip');
    const decompressedStream = compressedData.stream().pipeThrough(ds);

    const reader = decompressedStream.getReader();
    const chunks = [];
    let chunk;

    while ((chunk = await reader.read()) && !chunk.done) {
        chunks.push(chunk.value);
    }

    // 解凍されたデータを文字列に変換する
    const decoder = new TextDecoder();
    const decompressedString = chunks.map(chunk => decoder.decode(chunk, { stream: true })).join('');

    return decompressedString;
}

const sleep_time = 50

let cells = document.getElementById('othello-board');
const black = 1;
const white = 2;
const boardSize = 8;
const putAblePlace = 3;

let enableAI = false;
let IsAiWhite = false;
let AiLevel = 0;

let app = App;


// ページの読み込みが完了した後にinitialize関数を実行
window.addEventListener('DOMContentLoaded', async (event) => {

    // 初期値
    const selectLevel = document.getElementById('selectLevel');
    if (localStorage.getItem("selectLevel")) {
        selectLevel.value = localStorage.getItem("selectLevel");
    } else {
        selectLevel.value = 3;
    }
    const toggleFirstOrLast = document.getElementById('toggle_first_or_last');
    if (localStorage.getItem("toggleFirstOrLast")) {
        toggleFirstOrLast.checked = JSON.parse(localStorage.getItem("toggleFirstOrLast"));
    };

    await initializeOthello();
    initializeStyle();
    drawBoard();
});

function initializeStyle() {
    const aiBattleCheckbox = document.getElementById('toggle');
    const aiSetting = document.getElementById('aiSetting');
    const toggleFirstOrLast = document.getElementById('toggle_first_or_last');
    const textFirstOrLast = document.getElementById('text_first_or_last');
    const selectLevel = document.getElementById('selectLevel');
    const startButtonP = document.getElementById("startButtonP");
    const startButton = document.getElementById("startButton");
    const overlay = document.getElementById('ai-thinking-overlay');

    overlay.style.display = "block";


    selectLevel.addEventListener('change', () => {
        localStorage.setItem("selectLevel", selectLevel.value);
    })

    aiBattleCheckbox.addEventListener('change', function () {
        if (aiBattleCheckbox.checked) {
            startButtonP.style.transform = 'translateY(0px)';
            aiSetting.style.opacity = 1;
            toggleFirstOrLast.removeAttribute('disabled');
            selectLevel.removeAttribute('disabled');
            toggleFirstOrLast.style.visibility = "";
            selectLevel.style.visibility = "";
        } else {
            aiSetting.style.opacity = 0;
            startButtonP.style.transform = 'translateY(-30px)';
            toggleFirstOrLast.setAttribute('disabled', 'disabled');
            selectLevel.setAttribute('disabled', 'disabled');
            toggleFirstOrLast.style.visibility = "hidden";
            selectLevel.style.visibility = "hidden";
        }
    });

    toggleFirstOrLast.addEventListener('change', function () {
        if (toggleFirstOrLast.checked) {
            textFirstOrLast.textContent = "後攻"
        } else {
            textFirstOrLast.textContent = "先攻"
        }
        localStorage.setItem("toggleFirstOrLast", toggleFirstOrLast.checked.toString())
    });

    startButton.textContent = "Game Start"
    
    startButton.addEventListener('click', function () {
        const setup_modal = document.getElementById("setup_modal");
        setup_modal.style.display = 'none';
        overlay.style.display = "none";
        AiLevel = selectLevel.selectedIndex;
        app.set_level(AiLevel);

        enableAI = aiBattleCheckbox.checked;
        IsAiWhite = toggleFirstOrLast.checked;
        if (enableAI) {
            if (IsAiWhite) {
                app.put(4, 5);
                drawBoard();
            }
        }
    });
}


const sleep = waitTime => new Promise(resolve => setTimeout(resolve, waitTime));

async function initializeOthello() {
    await __wbg_init();

    app = new App();

    try {
        {
            const response = await fetch('./deft_eval_2024-01-27.json.gz');
            if (!response.ok) {
                throw new Error('ファイルの読み込みに失敗しました');
            }
            const data = await response.blob();
            const decompressedData = await decompressGzip(data);
    
            console.log("set evaluator");
            app.set_evaluator(decompressedData);
        }
    } catch (error) {
        alert("評価データの読み込みに失敗しました");
        alert(error);
        console.error('エラー:', error);
    }
    await sleep(sleep_time); // 100ms停止
}

function drawBoard({ showPutAblePlaces = true } = {}) {
    let b = app.get_board();
    let board = b.board;

    while (cells.firstChild) {
        cells.removeChild(cells.firstChild);
    }

    for (let y = 0; y < boardSize; y++) {
        const tr = document.createElement('tr');

        for (let x = 0; x < boardSize; x++) {
            const td = document.createElement('td');

            if (board[y][x] === white) {
                td.classList.add('white');
            } else if (board[y][x] === black) {
                td.classList.add('black');
            } else if (showPutAblePlaces && board[y][x] === putAblePlace) {
                td.classList.add('put-able-place');
                td.addEventListener('click', async () => {
                    await clickEvent(x, y);
                });
            } else {
                td.style.pointerEvents = "none";
            }

            tr.appendChild(td);
        }
        cells.appendChild(tr);
    }
    drawStatus(b);
}


function drawStatus(board) {

    let white_count = 0;
    let black_count = 0;
    
    for(let y = 0; y < 8; ++y){
        for(let x = 0; x < 8; ++x){
            if (board.board[y][x] == white) {
                ++white_count;
            }else if (board.board[y][x] == black) {
                ++black_count;
            }
        }        
    }
    let next_turn = "";
    if (board.next_turn == 0) {
        next_turn = "Black";
    } else if(board.next_turn == 0) {
        next_turn = "White";
    } else {
        next_turn = board.next_turn;
    }
    set_othello_status_prog(white_count + black_count, black_count)
    let status_html_element = document.getElementById("othello-status-prog");
    
    let black_count_html_element = document.getElementById("black-count");
    black_count_html_element.textContent=`${black_count}`;

    
    let white_count_html_element = document.getElementById("white-count");
    white_count_html_element.innerHTML=`${white_count}`;
    if (white_count >= 10){
        white_count_html_element.style.width = "6%";
    }else {
        white_count_html_element.style.width = "4%";
    }


}

async function set_othello_status_prog(max, value){
    let status_html_element = document.getElementById("othello-status-prog");
    status_html_element.max = max;
    status_html_element.value = value;
}

async function clickEvent(x, y) {
    const overlay = document.getElementById('ai-thinking-overlay');
    drawBoard({showPutAblePlaces: false});
    overlay.style.display = 'block';
    await repaint();
    await handleCellClick(x, y);
    overlay.style.display = 'none';
    drawBoard();
}


async function handleCellClick(x, y) {
    const put_ok = app.put(y, x);
    if (!put_ok) {
        return;
    }

    drawBoard({showPutAblePlaces: false});
    await repaint();
    await sleep(sleep_time);

    if(app.is_end_game()) {
        end_game();
        return;
    }

    if (app.is_no_put_place()) {
        alert("パスです。（置ける場所がありません。）");
        app.pass();
        return;
    }

    if (enableAI) {
        app.ai_put();
        
        while (app.is_no_put_place() && !app.is_end_game()) {
            drawBoard({showPutAblePlaces: false});
            await repaint();
            await sleep(sleep_time);
            alert("パスです。（置ける場所がありません。）");
            app.pass();
            app.ai_put();
        }
        if (app.is_end_game()) {
            drawBoard({ showPutAblePlaces: false });
            await repaint();
            await sleep(sleep_time);
            end_game();
            return;
        }  
    }


}
const repaint = async () => {
    for (let i = 0; i < 2; i++) {
        await new Promise(resolve => requestAnimationFrame(resolve));
    }
};

function end_game() {
    let board = app.get_board();
    let white_count = 0;
    let black_count = 0;
    
    for(let y = 0; y < 8; ++y){
        for(let x = 0; x < 8; ++x){
            if (board.board[y][x] == white) {
                ++white_count;
            }else if (board.board[y][x] == black) {
                ++black_count;
            }
        }        
    }

    let diff = black_count - white_count;
    let comment = "";


    if (enableAI) {
        if (IsAiWhite) diff = -diff;
        if (diff > 0) {
            comment = "あなたの勝ちです。";
        } else if (diff < 0) {
            comment = "AIの勝ちです。"
        } else {
            comment = "引き分けです。";
        }
        comment += `\n(AI: Level ${AiLevel})`;
    } else {
        if (diff > 0) {
            comment = "黒の勝ちです。";
        } else if (diff < 0)  {
            comment = "白の勝ちです。";
        } else {
            comment = "引き分けです。";
        }
    }

    if (Math.abs(diff) < 10) {
        comment += "\n\nGood Game !";
    }
    alert(comment);
    
}