/**
 * @fileoverview OpeningService - 定石機能を管理するサービス
 */

import { notationToPosition } from '../domain/types.js';
import { OPENINGS } from '../ui/openings.js';

/**
 * 定石データ
 * @typedef {Object} OpeningData
 * @property {string} name 定石名
 * @property {string} sequence 手順文字列（例: "F5D6C3D3C4"）
 * @property {number[]} positions 手順のposition配列
 */

/**
 * 定石マッチング結果
 * @typedef {Object} OpeningMatch
 * @property {string} name 現在の定石名
 * @property {number | null} nextPosition 次の推奨手
 */

/**
 * 定石機能を管理するサービス
 */
export class OpeningService {
    constructor() {
        /** @private @type {Map<number, OpeningData>} */
        this._openings = new Map();
        /** @private */
        this._loaded = false;
    }

    /**
     * 定石データをロード
     * @param {string} openingText opening.txtの内容
     */
    load(openingText) {
        this._openings = this._parseOpenings(openingText);
        this._loaded = true;
    }

    /**
     * ロード済みかどうか
     * @returns {boolean}
     */
    get isLoaded() {
        return this._loaded;
    }

    /**
     * opening.txtをパース
     * @private
     * @param {string} text
     * @returns {Map<number, OpeningData>}
     */
    _parseOpenings(text) {
        const result = new Map();
        const lines = text.split('\n');

        for (const line of lines) {
            // コメント行をスキップ
            const trimmed = line.replace(/^\s+/, '').replace(/^　+/, ''); // 全角・半角空白を除去
            if (trimmed.startsWith('//') || !trimmed.includes('=')) {
                continue;
            }

            // "定石名 = 手順" をパース
            const eqIndex = trimmed.indexOf('=');
            if (eqIndex === -1) continue;

            const name = trimmed.substring(0, eqIndex).trim();
            const sequence = trimmed.substring(eqIndex + 1).trim().toUpperCase();

            // OPENINGSリストから対応するインデックスを検索
            const openingEntry = OPENINGS.find(([, n]) => n === name);
            if (!openingEntry) continue;

            const index = openingEntry[0];

            // 手順をposition配列に変換
            const positions = this._parseSequence(sequence);

            result.set(index, {
                name,
                sequence,
                positions,
            });
        }

        return result;
    }

    /**
     * 手順文字列をposition配列に変換
     * @private
     * @param {string} sequence 例: "F5D6C3D3C4"
     * @returns {number[]}
     */
    _parseSequence(sequence) {
        const positions = [];
        // 2文字ずつ切り出し（例: F5, D6, C3...）
        for (let i = 0; i < sequence.length; i += 2) {
            if (i + 1 >= sequence.length) break;
            const notation = sequence.substring(i, i + 2);
            const pos = notationToPosition(notation.toLowerCase());
            if (pos !== null) {
                positions.push(pos);
            }
        }
        return positions;
    }

    /**
     * 現在の棋譜と選択された定石をマッチング
     * @param {number | null} selectedOpeningIndex 選択された定石のインデックス
     * @param {string[]} recordMoves 現在の棋譜（例: ["f5", "d6", "c3"]）
     * @returns {OpeningMatch | null}
     */
    match(selectedOpeningIndex, recordMoves) {
        if (selectedOpeningIndex === null || selectedOpeningIndex === undefined) {
            return null;
        }

        const opening = this._openings.get(selectedOpeningIndex);
        if (!opening) {
            return null;
        }

        // 棋譜から着手位置を抽出（passは無視）
        const playedPositions = recordMoves
            .filter((m) => m !== 'pass')
            .map((notation) => notationToPosition(notation))
            .filter((pos) => pos !== null);

        // 定石の手順と比較
        const openingPositions = opening.positions;

        // 現在の棋譜が定石の先頭部分と一致するかチェック
        for (let i = 0; i < playedPositions.length; i++) {
            if (i >= openingPositions.length) {
                // 定石を超えて進んでいる
                return null;
            }
            if (playedPositions[i] !== openingPositions[i]) {
                // 定石から外れた
                return null;
            }
        }

        // マッチしている場合、次の手を返す
        const nextIndex = playedPositions.length;
        if (nextIndex >= openingPositions.length) {
            // 定石を完走した
            return {
                name: opening.name,
                nextPosition: null,
            };
        }

        return {
            name: opening.name,
            nextPosition: openingPositions[nextIndex],
        };
    }

}
