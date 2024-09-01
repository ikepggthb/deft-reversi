import __wbg_init, { App } from "./pkg/deft_web.js";
import { Board } from "./board.js";


async function initializeOthello() {
    await __wbg_init();

    let app = new App();

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
    
            console.log("set evaluator");
            app.set_evaluator(decompressedData);
    } catch (error) {
        alert("評価データの読み込みに失敗しました\n" + error);
        console.error('エラー:', error);
    }

    return app;
}

//let app = initializeOthello();

let board = new Board();