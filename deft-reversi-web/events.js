/**
 * カスタムイベントの登録と発行を管理するシンプルなイベントディスパッチャクラス。
 * アプリケーション内の異なるモジュール間の疎結合な通信を可能にします。
 */
export class EventDispatcher {
    /**
     * EventDispatcherの新しいインスタンスを生成します。
     */
    constructor() {
        this.listeners = {};
    }

    /**
     * 指定されたイベントにリスナー（コールバック関数）を登録します。
     * @param {string} event リッスンするイベントの名前。
     * @param {Function} callback イベントが発行されたときに呼び出される関数。
     */
    addEventListener(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
    }

    /**
     * 指定されたイベントを発行し、登録されているすべてのリスナーを呼び出します。
     * @param {string} event 発行するイベントの名前。
     * @param {...any} data リスナーに渡すデータ。
     */
    dispatchEvent(event, ...data) {
        if (this.listeners[event]) {
            this.listeners[event].forEach((callback) => callback(...data));
        }
    }
}