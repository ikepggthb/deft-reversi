import pako from 'https://cdnjs.cloudflare.com/ajax/libs/pako/2.1.0/pako.esm.mjs';
import __wbg_init, { App } from "./pkg/deft_reversi_web.js";


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
    
            self.postMessage("set evaluator");
            app.set_evaluator(decompressedData);

    } catch (error) {
        self.postMessage("評価データの読み込みに失敗しました\n" + error);
    }


    self.postMessage("Ok");
    return app;
}



let app = initializeOthello();