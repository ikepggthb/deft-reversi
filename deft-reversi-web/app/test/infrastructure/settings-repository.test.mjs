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
const { SettingsRepository } = await import('../../infrastructure/settings-repository.js');

const DEFAULT_SETTINGS = Object.freeze({
    aiEnabled: true,
    aiLevel: 5,
    aiTurn: 'white',
    humanOpening: null,
});

test.beforeEach(() => {
    globalThis.localStorage.clear();
});

// === load ===

test('SettingsRepository.load: returns defaults when storage is empty', () => {
    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.deepEqual(settings, DEFAULT_SETTINGS);
});

test('SettingsRepository.load: loads and normalizes stored settings', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: false,
        aiLevel: 10,
        aiTurn: 'black',
        humanOpening: 3,
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.aiEnabled, false);
    assert.equal(settings.aiLevel, 10);
    assert.equal(settings.aiTurn, 'black');
    assert.equal(settings.humanOpening, 3);
});

test('SettingsRepository.load: normalizes invalid values to defaults', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: 'not-boolean',
        aiLevel: 100, // out of range
        aiTurn: 'invalid',
        humanOpening: 'bad',
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.aiEnabled, DEFAULT_SETTINGS.aiEnabled);
    assert.equal(settings.aiLevel, DEFAULT_SETTINGS.aiLevel);
    assert.equal(settings.aiTurn, DEFAULT_SETTINGS.aiTurn);
    assert.equal(settings.humanOpening, null);
});

test('SettingsRepository.load: migrates from legacy key', () => {
    localStorage.setItem('gameSettings', JSON.stringify({
        aiEnabled: false,
        aiLevel: 8,
        aiTurn: 'black',
        humanOpening: null,
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.aiEnabled, false);
    assert.equal(settings.aiLevel, 8);

    // Should have migrated to new key
    assert.ok(localStorage.getItem('deft-reversi-settings'));
    // Legacy key should be removed
    assert.equal(localStorage.getItem('gameSettings'), null);
});

test('SettingsRepository.load: handles corrupted JSON gracefully', () => {
    localStorage.setItem('deft-reversi-settings', 'not valid json');

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.deepEqual(settings, DEFAULT_SETTINGS);
});

// === save ===

test('SettingsRepository.save: persists settings', () => {
    const repo = new SettingsRepository();
    const toSave = {
        aiEnabled: false,
        aiLevel: 12,
        aiTurn: 'black',
        humanOpening: 5,
    };

    repo.save(toSave);

    const stored = JSON.parse(localStorage.getItem('deft-reversi-settings'));
    assert.deepEqual(stored, toSave);
});

// === reset ===

test('SettingsRepository.reset: removes stored settings', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: false,
        aiLevel: 10,
        aiTurn: 'black',
        humanOpening: 3,
    }));

    const repo = new SettingsRepository();
    repo.reset();

    assert.equal(localStorage.getItem('deft-reversi-settings'), null);
});

// === humanOpening normalization ===

test('SettingsRepository.load: normalizes humanOpening "none" to null', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: true,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: 'none',
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.humanOpening, null);
});

test('SettingsRepository.load: normalizes humanOpening empty string to null', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: true,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: '',
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.humanOpening, null);
});

test('SettingsRepository.load: converts string number humanOpening to integer', () => {
    localStorage.setItem('deft-reversi-settings', JSON.stringify({
        aiEnabled: true,
        aiLevel: 5,
        aiTurn: 'white',
        humanOpening: '7',
    }));

    const repo = new SettingsRepository();
    const settings = repo.load(DEFAULT_SETTINGS);

    assert.equal(settings.humanOpening, 7);
});
