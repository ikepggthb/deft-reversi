import { describe, it, mock } from 'node:test';
import assert from 'node:assert';
import { GraphStateManager } from '../../ui/graph-state-manager.js';

describe('GraphStateManager', () => {
    describe('constructor', () => {
        it('creates initial state with correct analysis level', () => {
            const manager = new GraphStateManager(10);
            const graph = manager.mainGraph;

            assert.strictEqual(graph.level, 10);
            assert.strictEqual(graph.source, 'live');
            assert.deepStrictEqual(graph.series, [0]);
            assert.deepStrictEqual(graph.baseSeries, [0]);
            assert.strictEqual(graph.loading, false);
            assert.strictEqual(graph.error, null);
        });
    });

    describe('cloneState', () => {
        it('creates a deep copy of graph state', () => {
            const manager = new GraphStateManager(8);
            const original = manager.mainGraph;
            original.series = [1, 2, 3];
            original.record = ['f5', 'd6'];

            const cloned = manager.cloneState(original);

            assert.notStrictEqual(cloned, original);
            assert.notStrictEqual(cloned.series, original.series);
            assert.deepStrictEqual(cloned.series, original.series);
            assert.notStrictEqual(cloned.record, original.record);
            assert.deepStrictEqual(cloned.record, original.record);
        });
    });

    describe('activeGraph', () => {
        it('returns main graph when not in study mode', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.activeGraph, manager.mainGraph);
        });

        it('returns study graph when in study mode', () => {
            const manager = new GraphStateManager(8);
            manager.enterStudyMode(5);

            assert.strictEqual(manager.activeGraph, manager.studyGraph);
            assert.notStrictEqual(manager.activeGraph, manager.mainGraph);
        });
    });

    describe('enterStudyMode', () => {
        it('creates study graph from main graph', () => {
            const manager = new GraphStateManager(8);
            manager.mainGraph.series = [1, 2, 3, 4];

            manager.enterStudyMode(3);

            assert.strictEqual(manager.isStudyMode, true);
            assert.strictEqual(manager.studyBranchPly, 3);
            assert.notStrictEqual(manager.studyGraph, manager.mainGraph);
            assert.deepStrictEqual(manager.studyGraph?.series, [1, 2, 3, 4]);
        });

        it('does nothing if already in study mode', () => {
            const manager = new GraphStateManager(8);
            manager.enterStudyMode(3);
            const studyGraph = manager.studyGraph;

            manager.enterStudyMode(5);

            assert.strictEqual(manager.studyGraph, studyGraph);
            assert.strictEqual(manager.studyBranchPly, 3);
        });
    });

    describe('exitStudyMode', () => {
        it('clears study mode state', () => {
            const manager = new GraphStateManager(8);
            manager.enterStudyMode(3);

            manager.exitStudyMode();

            assert.strictEqual(manager.isStudyMode, false);
            assert.strictEqual(manager.studyGraph, null);
            assert.strictEqual(manager.studyBranchPly, null);
        });
    });

    describe('reset', () => {
        it('resets all state to initial values', () => {
            const manager = new GraphStateManager(8);
            manager.mainGraph.series = [1, 2, 3];
            manager.enterStudyMode(2);

            manager.reset();

            assert.strictEqual(manager.isStudyMode, false);
            assert.strictEqual(manager.studyGraph, null);
            assert.deepStrictEqual(manager.mainGraph.series, [0]);
        });
    });

    describe('setAnalysisLevel', () => {
        it('updates analysis level for live graph', () => {
            const manager = new GraphStateManager(8);

            manager.setAnalysisLevel(12);

            assert.strictEqual(manager.mainGraph.level, 12);
        });

        it('does not update level for analysis graph', () => {
            const manager = new GraphStateManager(8);
            manager.mainGraph.source = 'analysis';

            manager.setAnalysisLevel(12);

            assert.strictEqual(manager.mainGraph.level, 8);
        });
    });

    describe('getCursor', () => {
        it('returns cursor for live graph with matching record', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.series = [0, 1, 2, 3];
            graph.record = ['f5', 'd6', 'c3'];

            const cursor = manager.getCursor(graph, ['f5', 'd6']);

            assert.strictEqual(cursor, 2);
        });

        it('returns null for non-matching record in live mode', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.record = ['f5', 'd6'];

            const cursor = manager.getCursor(graph, ['e6', 'f4']);

            assert.strictEqual(cursor, null);
        });

        it('returns cursor for analysis graph with matching prefix', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'analysis';
            graph.series = [0, 1, 2, 3, 4];
            graph.record = ['f5', 'd6', 'c3', 'c4'];

            const cursor = manager.getCursor(graph, ['f5', 'd6']);

            assert.strictEqual(cursor, 2);
        });
    });

    describe('containsCurrentRecord', () => {
        it('returns true for exact analysis record', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'analysis';
            graph.series = [0, 1, 2];
            graph.record = ['f5', 'd6'];

            assert.strictEqual(manager.containsCurrentRecord(graph, ['f5', 'd6']), true);
        });

        it('returns true for appended current-eval state', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'analysis';
            graph.record = ['f5', 'd6'];
            graph.currentEvalKey = 'f5 d6 c3|8';
            graph.currentEvalValue = 12;

            assert.strictEqual(manager.containsCurrentRecord(graph, ['f5', 'd6', 'c3']), true);
        });

        it('returns false for unrelated record', () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'analysis';
            graph.record = ['f5', 'd6'];

            assert.strictEqual(manager.containsCurrentRecord(graph, ['e6', 'f4']), false);
        });
    });

    describe('isPrefix', () => {
        it('returns true for valid prefix', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.isPrefix(['f5', 'd6'], ['f5', 'd6', 'c3']), true);
        });

        it('returns false for non-prefix', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.isPrefix(['f5', 'c4'], ['f5', 'd6', 'c3']), false);
        });

        it('returns true for exact match', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.isPrefix(['f5', 'd6'], ['f5', 'd6']), true);
        });

        it('returns true for empty prefix', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.isPrefix([], ['f5', 'd6']), true);
        });

        it('returns false if prefix is longer than record', () => {
            const manager = new GraphStateManager(8);

            assert.strictEqual(manager.isPrefix(['f5', 'd6', 'c3'], ['f5', 'd6']), false);
        });
    });

    describe('normalizePassSeries', () => {
        it('returns [0] for empty series', () => {
            const manager = new GraphStateManager(8);

            const result = manager.normalizePassSeries(['f5'], []);

            assert.deepStrictEqual(result, [0]);
        });

        it('copies previous value for pass moves', () => {
            const manager = new GraphStateManager(8);
            const record = ['f5', 'pass', 'd6'];
            const series = [0, 10, 5, 8];

            const result = manager.normalizePassSeries(record, series);

            assert.deepStrictEqual(result, [0, 10, 10, 8]);
        });

        it('handles no pass in record', () => {
            const manager = new GraphStateManager(8);
            const record = ['f5', 'd6', 'c3'];
            const series = [0, 10, 5, 8];

            const result = manager.normalizePassSeries(record, series);

            assert.deepStrictEqual(result, [0, 10, 5, 8]);
        });
    });

    describe('resolveStudyBranchPly', () => {
        it('returns divergence point', () => {
            const manager = new GraphStateManager(8);
            const mainRecord = ['f5', 'd6', 'c3', 'c4'];
            const studyRecord = ['f5', 'd6', 'e3', 'f4'];

            const ply = manager.resolveStudyBranchPly(mainRecord, studyRecord);

            assert.strictEqual(ply, 2);
        });

        it('returns 0 for completely different records', () => {
            const manager = new GraphStateManager(8);

            const ply = manager.resolveStudyBranchPly(['f5'], ['e6']);

            assert.strictEqual(ply, 0);
        });

        it('returns full length for identical records', () => {
            const manager = new GraphStateManager(8);
            const record = ['f5', 'd6', 'c3'];

            const ply = manager.resolveStudyBranchPly(record, [...record]);

            assert.strictEqual(ply, 3);
        });
    });

    describe('syncGraphState', () => {
        it('sets series to [0] when no analysis', async () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.hasAnalysis = false;
            graph.series = [1, 2, 3];

            await manager.syncGraphState(graph, [], async () => ({ ok: true, evalBlack: 0 }), 8, true, () => {});

            assert.deepStrictEqual(graph.series, [0]);
            assert.strictEqual(graph.loading, false);
        });

        it('does not fetch if engine not ready', async () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.record = [];
            const evaluatePosition = mock.fn(async () => ({ ok: true, evalBlack: 10 }));

            await manager.syncGraphState(graph, ['f5'], evaluatePosition, 8, false, () => {});

            assert.strictEqual(evaluatePosition.mock.calls.length, 0);
        });

        it('keeps full live series when navigating backward inside the plotted line', async () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'live';
            graph.record = ['f5', 'd6', 'c3'];
            graph.baseSeries = [0, 10, 8, 12];
            graph.series = [0, 10, 8, 12];
            const evaluatePosition = mock.fn(async () => ({ ok: true, evalBlack: 99 }));

            await manager.syncGraphState(graph, ['f5'], evaluatePosition, 8, true, () => {});

            assert.deepStrictEqual(graph.record, ['f5', 'd6', 'c3']);
            assert.deepStrictEqual(graph.baseSeries, [0, 10, 8, 12]);
            assert.deepStrictEqual(graph.series, [0, 10, 8, 12]);
            assert.strictEqual(graph.loading, false);
            assert.strictEqual(evaluatePosition.mock.calls.length, 0);
        });

        it('extends live series again when redoing after backward navigation', async () => {
            const manager = new GraphStateManager(8);
            const graph = manager.mainGraph;
            graph.source = 'live';
            graph.record = ['f5', 'd6', 'c3'];
            graph.baseSeries = [0, 10, 8, 12];
            graph.series = [0, 10, 8, 12];

            await manager.syncGraphState(graph, ['f5', 'd6'], async () => ({ ok: true, evalBlack: 99 }), 8, true, () => {});
            await manager.syncGraphState(graph, ['f5', 'd6', 'c3'], async () => ({ ok: true, evalBlack: 99 }), 8, true, () => {});

            assert.deepStrictEqual(graph.record, ['f5', 'd6', 'c3']);
            assert.deepStrictEqual(graph.baseSeries, [0, 10, 8, 12]);
            assert.deepStrictEqual(graph.series, [0, 10, 8, 12]);
            assert.strictEqual(graph.loading, false);
        });
    });
});
