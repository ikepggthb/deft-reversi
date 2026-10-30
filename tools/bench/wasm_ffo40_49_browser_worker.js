// Runs inside a fresh browser Worker; inputs are supplied by the Node harness.
self.onmessage = async ({ data }) => {
    try {
        const module = await import(data.moduleUrl);
        await module.default();
        const response = await fetch(data.modelUrl);
        if (!response.ok) throw new Error(`Model HTTP ${response.status}`);
        const bytes = new Uint8Array(await new Response(response.body.pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
        const engine = new module.WasmEngine(bytes, 32);
        let totalMs = 0;
        let totalNodes = 0n;
        for (const item of data.positions) {
            const position = module.WasmPosition.fromString(item.position);
            engine.clearTt();
            const result = engine.search(position, { goal: { kind: 'endgame' } });
            position.free();
            const score = Math.max(...item.answers.map(answer => answer.score));
            if (!result.solved || result.stopReason !== 'Completed' || result.score !== score ||
                !item.answers.some(answer => answer.move === result.bestMove && answer.score === score)) {
                throw new Error(`FFO ${item.number}: ${JSON.stringify(result)}`);
            }
            totalMs += result.elapsedMs;
            totalNodes += BigInt(result.nodes);
            self.postMessage({ type: 'position', variant: data.variant, number: item.number,
                elapsedMs: result.elapsedMs, nodes: result.nodes, score: result.score, bestMove: result.bestMove });
        }
        engine.free();
        self.postMessage({ type: 'done', variant: data.variant, totalMs, totalNodes: totalNodes.toString() });
    } catch (error) {
        self.postMessage({ type: 'error', message: String(error) });
    }
};
