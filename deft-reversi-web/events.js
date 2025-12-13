export class EventDispatcher {
    constructor() {
        this.listeners = {};
    }

    addEventListener(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
    }

    dispatchEvent(event, ...data) {
        if (this.listeners[event]) {
            this.listeners[event].forEach((callback) => callback(...data));
        }
    }
}

