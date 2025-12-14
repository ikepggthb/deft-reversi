const DEFAULT_TIMEOUT_MS = 120000;

class WorkerWrapper {
    constructor(workerFile) {
        this.worker = new Worker(workerFile, { type: "module" });
        this.pendingRequests = new Map();
        this.generateRequestId = () => `${Date.now()}-${Math.random()}`;
        this.attachMessageHandler();
    }

    attachMessageHandler() {
        this.worker.addEventListener("message", (event) => {
            if (!event?.data || typeof event.data !== "object") return;
            const { payload, requestId, ok, error } = event.data;
            if (!requestId) return;

            const pending = this.pendingRequests.get(requestId);
            if (!pending) return;

            if (ok === false) {
                pending.reject(new Error(error ?? "Unknown worker error"));
            } else {
                pending.resolve(payload);
            }
            this.pendingRequests.delete(requestId);
        });
    }

    sendRequest(type, ...payload) {
        return new Promise((resolve, reject) => {
            const requestId = this.generateRequestId();
            const timeoutId = setTimeout(() => {
                this.pendingRequests.delete(requestId);
                reject(new Error(`Worker request timeout: ${type}`));
            }, DEFAULT_TIMEOUT_MS);

            this.pendingRequests.set(requestId, { resolve: (v) => { clearTimeout(timeoutId); resolve(v); }, reject: (e) => { clearTimeout(timeoutId); reject(e); } });
            this.worker.postMessage({ type, payload, requestId });
        });
    }
}

export class Engine extends WorkerWrapper {
    constructor() {
        super("engine.js");
        this.initializeMethods([
            "solveTurn",
        ]);
    }

    initializeMethods(methods) {
        methods.forEach((method) => {
            this[method] = (...payload) => this.sendRequest(method, ...payload);
        });
    }
}
