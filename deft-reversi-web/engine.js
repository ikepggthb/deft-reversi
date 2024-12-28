import pako from 'https://cdnjs.cloudflare.com/ajax/libs/pako/2.1.0/pako.esm.mjs';
import __wbg_init, { App } from "./pkg/deft_reversi_web.js";

async function fetch_eval_data() {

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
        console.log("評価データの読み込みに失敗しました\n" + error);
    }
}


async function fetch_opening_data() {

    try {
        const response = await fetch('./opening.txt');
        if (!response.ok) {
            throw new Error(response.statusText);
        }
        const text = await response.text();
        return text;

    } catch (error) {
        console.log("評価データの読み込みに失敗しました\n" + error);
    }
}

async function init() {
    await __wbg_init();
    console.log("fetch eval data");
    const eval_data = await fetch_eval_data();
    const oopening_data = await fetch_opening_data();

    console.log("set evaluator");
    const app = await App.new(eval_data, oopening_data);
    
    self.postMessage("ready");
    return app;
}

const app = await init();

self.addEventListener('message', (event) => {
    if (app === undefined) return;
    const { type, payload, requestId } = event.data;

    const result = (function (){
        switch (type) {
            case 'isLegalMove':
                return app.is_legal_move(...payload);
            case 'getState':
                return app.get_state(...payload);
            case 'isPass':
                return app.is_pass();
            case 'pass':
                return app.pass();
            case 'isEnd':
                return app.is_end();
            case 'put':
                return app.put(...payload);
            case 'aiPut':
                return app.ai_put(...payload);
            case  'undo':
                return app.undo();
            case 'redo':
                return app.redo();
            case 'getRecord':
                return app.get_record(...payload);
            case 'newGame' :
                return app.new_game();
            case 'setHumanOpening':
                return app.set_human_opening(...payload);
            default:
                console.warn(`Unknown message type: ${type}`);
                return;
        }
    })();

    self.postMessage({
        payload: result,
        requestId
    });
});