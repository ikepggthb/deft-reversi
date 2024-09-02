import { Board } from "./board.js";


const worker = new Worker('engine.js', { type: 'module' });

// メッセージを受信してコンソールに表示する
worker.addEventListener('message', (message) => {
   console.log(message.data);
});


let board = new Board();