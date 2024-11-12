import pako from 'https://cdnjs.cloudflare.com/ajax/libs/pako/2.1.0/pako.esm.mjs';
import __wbg_init, { App } from "./pkg/deft_reversi_web.js";

let ai = true;

async function fetch_eval_data() {
    await __wbg_init();

    try {
        const response = await fetch('./deft_eval_2024-01-27.json.gz');
        if (!response.ok) {
            throw new Error(response.statusText);
        }

        const data = await response.arrayBuffer();
        const decompressedData = await pako.ungzip(data, { to: 'string' });

        // 将来的にこちらを使う
        // const data = await response.blob();
        // const decompressedData = await decompressGzip(data);

        return decompressedData;

    } catch (error) {
        self.postMessage({ type: "console_log", data: "評価データの読み込みに失敗しました\n" + error});
    }
}

let stopDrawMoveScores = false;
let draw_id = 0;
let draw_move_scores_lv = 12;

function draw_move_scores(lv) {
    const id = draw_id;
    const start = lv % 4 == 0 ? 4 : lv % 4;
    for (let i = start; i <= lv; i += 4) {
        if (stopDrawMoveScores) break;
        if (id != draw_id) break;
        self.postMessage({ type: "console_log", data: "draw_move_scores lv: "+i })
        self.postMessage({ type: "render", status: app.get_state(i) });

    }
}


self.postMessage({type:"console_log", data:"fetch eval data"});
let eval_data = await fetch_eval_data();

self.postMessage({type: "console_log", data: "set evaluator"});
let app = await App.new(eval_data);


self.postMessage({type: "render", status: app.get_state(null)});

function put(item) {
    app.set_ai_level(10);
    if (!app.is_legal_move(item.position)) {
        console.log("can not put !");
        self.postMessage({ type: "thinking", isThinking: false });
        return;
    }
    app.put(item.position);
    self.postMessage({ type: "render", status: app.get_state(null) });
    if (app.is_end()) {
        self.postMessage({ type: "alert", message: "end!" });
        self.postMessage({ type: "thinking", isThinking: false });
        return;
    }
    if (app.is_pass()) {
        self.postMessage({ type: "alert", message: "パスです。" });
        app.pass();
        stopDrawMoveScores = false;
        draw_id++;
        draw_move_scores(draw_move_scores_lv);
        self.postMessage({ type: "thinking", isThinking: false });
        return;
    }

    if (ai == true) {
        app.ai_put();
        self.postMessage({ type: "render", status: app.get_state(null) });
        if (app.is_end()) {
            self.postMessage({ type: "alert", message: "end!" });
            self.postMessage({ type: "thinking", isThinking: false });
            return;
        }
        while (app.is_pass()) {
            self.postMessage({ type: "alert", message: "パスです。" });
            app.pass();
            app.ai_put();
            self.postMessage({ type: "render", status: app.get_state(null) });
            if (app.is_end()) {
                self.postMessage({ type: "alert", message: "end!" });
                self.postMessage({ type: "thinking", isThinking: false });
                return;
            }
        }
    }
    self.postMessage({ type: "thinking", isThinking: false });
    stopDrawMoveScores = false;
    draw_id++;
    draw_move_scores(draw_move_scores_lv);
}

function ai_vs_ai() {
    while(!app.is_end()){
        app.set_ai_level(22);
        app.ai_put();
        self.postMessage({ type: "render", status: app.get_state(null) });
        if (app.is_end()) {
            self.postMessage({ type: "alert", message: "end!" });
            self.postMessage({ type: "thinking", isThinking: false });
            return;
        }
        while (app.is_pass()) {
            self.postMessage({ type: "alert", message: "パスです。" });
            app.pass();
            app.ai_put();
            self.postMessage({ type: "render", status: app.get_state(null) });
            if (app.is_end()) {
                self.postMessage({ type: "alert", message: "end!" });
                self.postMessage({ type: "thinking", isThinking: false });
                return;
            }
        }
        stopDrawMoveScores = false;
        draw_id++;
        self.postMessage({ type: "render", status: app.get_state(null) });
        
        app.set_ai_level(3);
        app.ai_put();
        self.postMessage({ type: "render", status: app.get_state(null) });
        if (app.is_end()) {
            self.postMessage({ type: "alert", message: "end!" });
            self.postMessage({ type: "thinking", isThinking: false });
            return;
        }
        while (app.is_pass()) {
            self.postMessage({ type: "alert", message: "パスです。" });
            app.pass();
            app.ai_put();
            self.postMessage({ type: "render", status: app.get_state(null) });
            if (app.is_end()) {
                self.postMessage({ type: "alert", message: "end!" });
                self.postMessage({ type: "thinking", isThinking: false });
                return;
            }
        }
        stopDrawMoveScores = false;
        draw_id++;
        self.postMessage({ type: "render", status: app.get_state(null) });
    }
    self.postMessage({ type: "thinking", isThinking: false });
}



self.addEventListener('message', (message) => {
    let item = message.data;
    if (item.type == "put") {
        // put(item);
        ai_vs_ai();
    } else if (item.type == "draw_score") {
        let score = app.get_eval_scores();
        self.postMessage({ type: "console_log", data: score });
    } else if (item.type == "draw_move_scores") {
        // stopDrawMoveScores = false;
        // draw_move_scores(16);
    } else if (item.type == "stop_draw_move_scores") {
        self.postMessage({ type: "console_log", data: "stop_draw_move_scores" })
        stopDrawMoveScores = true;
    }
});
