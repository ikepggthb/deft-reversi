import test from 'node:test';
import assert from 'node:assert/strict';
import { GameService } from '../../application/game-service.js';
import { positionToNotation } from '../../domain/types.js';

class StubSettingsService {
    constructor() {
        this.enableAi = false;
        this.aiLevel = 5;
        this.analysisLevel = 7;
        this.analysisRunLevel = 7;
        this.blackAiLevel = 5;
        this.whiteAiLevel = 5;
        this.aiTurn = 'white';
        this.humanOpening = null;
        this.enableHint = false;
        this.hintLevel = 7;
    }
    save() {}
    toggleHint() {
        this.enableHint = !this.enableHint;
    }
}

class StubAiEngine {
    constructor() {
        this.isReady = true;
    }
    initialize(onReady) {
        this._onReady = onReady;
    }
    terminate() {}
    async solveTurn() {
        return { bestMove: null, eval: 0 };
    }
}

class StubHintService {
    cancel() {}
    scheduleRefresh(_board, _level, _isAiTurn, onUpdate) {
        onUpdate(null);
    }
}

class StubOpeningService {
    get isLoaded() {
        return false;
    }
    match() {
        return null;
    }
}

class StubUI {
    constructor() {
        this.infoLogs = [];
        this.errorLogs = [];
        this.endGameCalls = [];
        this.renderCalls = [];
        this.passCount = 0;
    }
    render(viewModel, blackName, whiteName) {
        this.renderCalls.push({ viewModel, blackName, whiteName });
    }
    onAIReady() {}
    logInfo(message) {
        this.infoLogs.push(String(message));
    }
    logError(message) {
        this.errorLogs.push(String(message));
    }
    drawPassMessage() {
        this.passCount += 1;
    }
    showEndGameModal(blackCount, whiteCount, blackName, whiteName, record) {
        this.endGameCalls.push({ blackCount, whiteCount, blackName, whiteName, record });
    }
}

async function settle(service) {
    await service._queue;
}

function createService(options = {}) {
    const ui = options.ui ?? new StubUI();
    const settings = options.settingsService ?? new StubSettingsService();
    const service = new GameService({
        ui,
        settingsService: settings,
        aiEngine: options.aiEngine ?? new StubAiEngine(),
        hintService: options.hintService ?? new StubHintService(),
        openingService: options.openingService ?? new StubOpeningService(),
        loadOpeningData: false,
        autoStart: false,
    });
    return { service, ui, settings };
}

const FULL_GAME_RECORD = [
    'd3', 'c5', 'f6', 'f5', 'e6', 'f7', 'c6', 'c7', 'b5', 'a4', 'g6', 'h5', 'f8', 'c4', 'b6', 'g5', 'f4',
    'e7', 'a5', 'a6', 'd6', 'd8', 'e8', 'g8', 'b7', 'b4', 'c8', 'd7', 'h8', 'a8', 'h6', 'h4', 'g7', 'c2',
    'e3', 'g4', 'f3', 'h7', 'g3', 'd2', 'h3', 'g2', 'h2', 'pass', 'b3', 'c3', 'a3', 'a2', 'b2', 'c1', 'a1',
    'b1', 'b8', 'pass', 'e1', 'e2', 'h1', 'g1', 'f1', 'pass', 'f2', 'pass', 'a7', 'pass', 'd1',
];

test('GameService scenario: clicking after game over shows result modal instead of AI-turn message', async () => {
    const { service, ui } = createService();
    service.startSession({
        aiEnabled: true,
        aiLevel: 4,
        aiTurn: 'white',
        humanOpening: null,
        blackPlayerName: 'あなた',
        whitePlayerName: 'AI Lv 4',
        record: FULL_GAME_RECORD,
        enableHint: false,
        runAiOnStart: false,
        pendingAutoStart: false,
    });
    await settle(service);

    service.handleBoardClick(0);
    await settle(service);

    assert.equal(ui.endGameCalls.length, 1);
    assert.equal(ui.infoLogs.includes('AIの手番です。お待ちください。'), false);
});

test('GameService scenario: undo then alternate move replaces future branch', async () => {
    const { service } = createService();
    service.startSession({
        aiEnabled: false,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: null,
        record: ['f5', 'd6'],
        enableHint: false,
        runAiOnStart: false,
    });
    await settle(service);

    service.undo();
    await settle(service);

    const alternate = service._game.legalMovePositions.find((pos) => positionToNotation(pos) !== 'd6');
    assert.notEqual(alternate, undefined);
    service.handleBoardClick(alternate);
    await settle(service);

    assert.equal(service.record.length, 2);
    assert.equal(service.record[1], positionToNotation(alternate));
    assert.equal(service.canRedo, false);
});

test('GameService scenario: undoToStart and redoToEnd move across the active record', async () => {
    const { service } = createService();
    service.startSession({
        aiEnabled: false,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: null,
        record: ['f5', 'd6', 'c3'],
        enableHint: false,
        runAiOnStart: false,
    });
    await settle(service);

    service.undoToStart();
    await settle(service);
    assert.deepEqual(service.record, []);

    service.redoToEnd();
    await settle(service);
    assert.deepEqual(service.record, ['f5', 'd6', 'c3']);
});

test('GameService scenario: undo to AI turn requires explicit restart instead of AI-turn waiting message', async () => {
    const { service, ui } = createService();
    service.startSession({
        aiEnabled: true,
        aiLevel: 4,
        aiTurn: 'white',
        humanOpening: null,
        blackPlayerName: 'あなた',
        whitePlayerName: 'AI Lv 4',
        record: ['f5', 'd6'],
        enableHint: false,
        runAiOnStart: false,
        pendingAutoStart: false,
    });
    await settle(service);

    service.undo();
    await settle(service);

    assert.equal(service.isAutoStartPending, true);

    service.handleBoardClick(19);
    await settle(service);

    assert.equal(ui.infoLogs.at(-1), '対局開始を押すと AI が着手します。');
    assert.equal(ui.infoLogs.includes('AIの手番です。お待ちください。'), false);
});

test('GameService scenario: study mode allows branching on AI turn and returns to main line on exit', async () => {
    const { service, ui } = createService();
    service.startSession({
        aiEnabled: true,
        aiLevel: 4,
        aiTurn: 'white',
        humanOpening: null,
        blackPlayerName: 'あなた',
        whitePlayerName: 'AI Lv 4',
        record: ['f5'],
        enableHint: false,
        runAiOnStart: false,
        pendingAutoStart: false,
    });
    await settle(service);

    service.setStudyMode(true);
    await settle(service);

    const studyMove = service._game.legalMovePositions[0];
    service.handleBoardClick(studyMove);
    await settle(service);

    assert.equal(service.record.length, 2);
    assert.equal(ui.infoLogs.includes('AIの手番です。お待ちください。'), false);

    service.setStudyMode(false);
    await settle(service);

    assert.deepEqual(service.record, ['f5']);
});

test('GameService scenario: study-mode game over does not show result modal', async () => {
    const { service, ui } = createService();
    service.startSession({
        aiEnabled: false,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: null,
        record: FULL_GAME_RECORD,
        enableHint: false,
        runAiOnStart: false,
    });
    await settle(service);

    service.setStudyMode(true);
    await settle(service);
    service.handleBoardClick(0);
    await settle(service);

    assert.equal(ui.endGameCalls.length, 0);
});

test('GameService scenario: goToPly moves to requested position in active record', async () => {
    const { service } = createService();
    service.startSession({
        aiEnabled: false,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: null,
        record: ['f5', 'd6', 'c3'],
        enableHint: false,
        runAiOnStart: false,
    });
    await settle(service);

    service.goToPly(1);
    await settle(service);
    assert.deepEqual(service.record, ['f5']);

    service.goToPly(3);
    await settle(service);
    assert.deepEqual(service.record, ['f5', 'd6', 'c3']);
});

test('GameService scenario: analyzeRecord reports progress from start to finish', async () => {
    const { service } = createService();
    const progress = [];

    const result = await service.analyzeRecord(['f5', 'd6'], 7, {
        onProgress: ({ completed, total, currentPly }) => {
            progress.push({ completed, total, currentPly });
        },
    });

    assert.equal(result.ok, true);
    assert.deepEqual(progress, [
        { completed: 1, total: 3, currentPly: 2 },
        { completed: 2, total: 3, currentPly: 1 },
        { completed: 3, total: 3, currentPly: 0 },
    ]);
    assert.deepEqual(result.analysis.map((point) => point.ply), [0, 1, 2]);
});
