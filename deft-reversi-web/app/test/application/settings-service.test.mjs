import test from 'node:test';
import assert from 'node:assert/strict';

// localStorage mock
class MockStorage {
    constructor() {
        this._data = new Map();
    }
    getItem(key) {
        return this._data.get(key) ?? null;
    }
    setItem(key, value) {
        this._data.set(key, String(value));
    }
    removeItem(key) {
        this._data.delete(key);
    }
    clear() {
        this._data.clear();
    }
}

// Setup global localStorage mock before importing module
globalThis.localStorage = new MockStorage();

// Dynamic import after mock setup
const { SettingsService } = await import('../../application/settings-service.js');

test.beforeEach(() => {
    globalThis.localStorage.clear();
});

// === Initial state (defaults) ===

test('SettingsService: initializes with default values', () => {
    const service = new SettingsService();

    assert.equal(service.enableAi, true);
    assert.equal(service.aiLevel, 5);
    assert.equal(service.analysisLevel, 7);
    assert.equal(service.analysisRunLevel, 7);
    assert.equal(service.blackAiLevel, 5);
    assert.equal(service.whiteAiLevel, 5);
    assert.equal(service.aiTurn, 'white');
    assert.equal(service.humanOpening, null);
});

test('SettingsService: initializes hint settings (session-only)', () => {
    const service = new SettingsService();

    assert.equal(service.enableHint, false);
    assert.equal(service.hintLevel, 7);
});

// === Load from storage ===

test('SettingsService: loads stored settings on construction', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: false,
        aiLevel: 12,
        analysisLevel: 9,
        analysisRunLevel: 13,
        blackAiLevel: 11,
        whiteAiLevel: 13,
        aiTurn: 'black',
        humanOpening: 2,
    }));

    const service = new SettingsService();

    assert.equal(service.enableAi, false);
    assert.equal(service.aiLevel, 12);
    assert.equal(service.analysisLevel, 9);
    assert.equal(service.analysisRunLevel, 13);
    assert.equal(service.blackAiLevel, 11);
    assert.equal(service.whiteAiLevel, 13);
    assert.equal(service.aiTurn, 'black');
    assert.equal(service.humanOpening, 2);
});

// === Setters ===

test('SettingsService: enableAi setter updates value', () => {
    const service = new SettingsService();
    service.enableAi = false;
    assert.equal(service.enableAi, false);
});

test('SettingsService: aiLevel setter updates value', () => {
    const service = new SettingsService();
    service.aiLevel = 20;
    assert.equal(service.aiLevel, 20);
});

test('SettingsService: analysisLevel setter updates value', () => {
    const service = new SettingsService();
    service.analysisLevel = 14;
    assert.equal(service.analysisLevel, 14);
});

test('SettingsService: analysisRunLevel setter updates value', () => {
    const service = new SettingsService();
    service.analysisRunLevel = 15;
    assert.equal(service.analysisRunLevel, 15);
});

test('SettingsService: blackAiLevel setter updates value', () => {
    const service = new SettingsService();
    service.blackAiLevel = 16;
    assert.equal(service.blackAiLevel, 16);
});

test('SettingsService: whiteAiLevel setter updates value', () => {
    const service = new SettingsService();
    service.whiteAiLevel = 18;
    assert.equal(service.whiteAiLevel, 18);
});

test('SettingsService: aiTurn setter updates value', () => {
    const service = new SettingsService();
    service.aiTurn = 'black';
    assert.equal(service.aiTurn, 'black');
});

test('SettingsService: aiTurn setter accepts both', () => {
    const service = new SettingsService();
    service.aiTurn = 'both';
    assert.equal(service.aiTurn, 'both');
});

test('SettingsService: enableHint setter updates value', () => {
    const service = new SettingsService();
    service.enableHint = true;
    assert.equal(service.enableHint, true);
});

test('SettingsService: hintLevel setter updates value', () => {
    const service = new SettingsService();
    service.hintLevel = 15;
    assert.equal(service.hintLevel, 15);
});

test('SettingsService: humanOpening setter updates value', () => {
    const service = new SettingsService();
    service.humanOpening = 5;
    assert.equal(service.humanOpening, 5);
});

// === toggleHint ===

test('SettingsService.toggleHint: toggles hint state', () => {
    const service = new SettingsService();
    assert.equal(service.enableHint, false);

    service.toggleHint();
    assert.equal(service.enableHint, true);

    service.toggleHint();
    assert.equal(service.enableHint, false);
});

// === save ===

test('SettingsService.save: persists AI settings to storage', () => {
    const service = new SettingsService();
    service.enableAi = false;
    service.aiLevel = 18;
    service.analysisLevel = 10;
    service.analysisRunLevel = 12;
    service.blackAiLevel = 12;
    service.whiteAiLevel = 14;
    service.aiTurn = 'both';
    service.humanOpening = 3;

    service.save();

    const stored = JSON.parse(localStorage.getItem('deft-reversi-settings'));
    assert.equal(stored.aiEnabled, false);
    assert.equal(stored.aiLevel, 18);
    assert.equal(stored.analysisLevel, 10);
    assert.equal(stored.analysisRunLevel, 12);
    assert.equal(stored.blackAiLevel, 12);
    assert.equal(stored.whiteAiLevel, 14);
    assert.equal(stored.aiTurn, 'both');
    assert.equal(stored.humanOpening, 3);
});

test('SettingsService.save: does not persist hint settings', () => {
    const service = new SettingsService();
    service.enableHint = true;
    service.hintLevel = 20;

    service.save();

    const stored = JSON.parse(localStorage.getItem('deft-reversi-settings'));
    // Hint settings should not be in storage
    assert.equal(stored.enableHint, undefined);
    assert.equal(stored.hintLevel, undefined);
});
