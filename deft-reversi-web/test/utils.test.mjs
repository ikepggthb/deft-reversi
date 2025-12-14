import test from "node:test";
import assert from "node:assert/strict";
import {
    applyMoveForTurn,
    countBits,
    flipBits,
    isEnd,
    isPass,
    legalMoves,
} from "../utils.js";

const FULL_MASK = 0xffff_ffff_ffff_ffffn;
const INITIAL_BLACK = 0x0000000810000000n;
const INITIAL_WHITE = 0x0000001008000000n;

function maskOfPositions(positions) {
    return positions.reduce((acc, pos) => acc | (1n << BigInt(pos)), 0n);
}

test("legalMoves: initial position (black)", () => {
    const moves = legalMoves(INITIAL_BLACK, INITIAL_WHITE);
    const expected = maskOfPositions([19, 26, 37, 44]); // d3, c4, f5, e6
    assert.equal(moves, expected);
});

test("legalMoves: basic horizontal flip pattern", () => {
    // X O .  => legal at pos2
    const player = maskOfPositions([0]);
    const opponent = maskOfPositions([1]);
    const moves = legalMoves(player, opponent);
    assert.ok((moves & (1n << 2n)) !== 0n);
});

test("legalMoves: edge wrap is not allowed (h1 does not see a1)", () => {
    // X.......O.  (wrap bug would incorrectly allow b1)
    const player = maskOfPositions([7]); // h1
    const opponent = maskOfPositions([0]); // a1
    const moves = legalMoves(player, opponent);
    assert.equal(moves & (1n << 1n), 0n);
});

test("legalMoves: basic vertical flip pattern", () => {
    // column a: X at a1, O at a2 => legal at a3
    const player = maskOfPositions([0]);
    const opponent = maskOfPositions([8]);
    const moves = legalMoves(player, opponent);
    assert.ok((moves & (1n << 16n)) !== 0n);
});

test("legalMoves: basic diagonal flip pattern", () => {
    // diagonal: X at a1, O at b2 => legal at c3
    const player = maskOfPositions([0]);
    const opponent = maskOfPositions([9]);
    const moves = legalMoves(player, opponent);
    assert.ok((moves & (1n << 18n)) !== 0n);
});

test("applyMoveForTurn: initial black move flips correctly", () => {
    const pos = 37; // f5
    const { nextPlayer, nextOpponent, flipped } = applyMoveForTurn(
        INITIAL_BLACK,
        INITIAL_WHITE,
        pos,
    );
    assert.equal(countBits(nextPlayer), 4);
    assert.equal(countBits(nextOpponent), 1);

    const posMask = 1n << BigInt(pos);
    assert.equal(nextOpponent ^ INITIAL_WHITE, flipped);
    assert.equal(nextPlayer ^ INITIAL_BLACK, flipped ^ posMask);
    assert.ok((flipped & posMask) === 0n);
});

test("flipBits: returned mask equals opponent delta", () => {
    const pos = 37; // f5
    const posMask = 1n << BigInt(pos);
    const flipped = flipBits(posMask, INITIAL_BLACK, INITIAL_WHITE);
    const { nextOpponent } = applyMoveForTurn(INITIAL_BLACK, INITIAL_WHITE, pos);
    assert.equal(nextOpponent ^ INITIAL_WHITE, flipped);
});

test("random invariants: legal moves subset of empty squares", () => {
    let seed = 0x12345678;
    function rnd() {
        seed = (seed * 1664525 + 1013904223) >>> 0;
        return seed;
    }

    for (let i = 0; i < 2000; i++) {
        const a = BigInt(rnd()) | (BigInt(rnd()) << 32n);
        const b = BigInt(rnd()) | (BigInt(rnd()) << 32n);
        const player = a & FULL_MASK;
        const opponent = (b & ~player) & FULL_MASK;

        const moves = legalMoves(player, opponent);
        const empty = ~(player | opponent) & FULL_MASK;
        assert.equal(moves & ~empty, 0n);
    }
});

test("find a pass position: isPass true implies opponent has moves", () => {
    let seed = 0xa5a5a5a5;
    function rnd() {
        seed = (seed * 1103515245 + 12345) >>> 0;
        return seed;
    }

    for (let i = 0; i < 50000; i++) {
        const a = BigInt(rnd()) | (BigInt(rnd()) << 32n);
        const b = BigInt(rnd()) | (BigInt(rnd()) << 32n);
        const player = a & FULL_MASK;
        const opponent = (b & ~player) & FULL_MASK;
        if (player === 0n || opponent === 0n) continue;

        if (isPass(player, opponent)) {
            assert.equal(legalMoves(player, opponent), 0n);
            assert.notEqual(legalMoves(opponent, player), 0n);
            assert.equal(isEnd(player, opponent), false);
            return;
        }
    }
    assert.fail("Could not find a pass position; likely bug in legalMoves/isPass");
});

