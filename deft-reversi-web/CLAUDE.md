# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Run all tests
cd app && npm test

# Run a single test file
cd app && node --test test/domain/board.test.mjs
```

## Architecture

Deft Reversi Web is a Reversi AI game. Heavy AI computation runs in a Web Worker + WASM to keep the UI responsive.

### Layer Structure

```
UI (ui/*.js)
  ↓ dispatchEvent / render(viewModel)
Application (application/*.js)
  ↓ apply moves / query state
Domain (domain/*.js)
  ↓
Infrastructure (infrastructure/*.js)
  ↓ postMessage
Web Worker (engine/engine.js)
  ↓
WASM (Rust)
```

### Key Components

- **GameService** (`application/game-service.js`): Orchestrator. Manages game state, event serialization via `_runExclusive()`, AI turns, undo/redo, and hint jobs. Converts domain state to ViewModel for UI.
- **Board/Game** (`domain/`): Immutable value objects using `BigInt` bitboard (64-bit). All state transitions create new instances.
- **AiEngine** (`infrastructure/ai-engine.js`): Worker RPC + bitboard conversion (`BigInt` ↔ `u32 low/high`). Application layer calls `solveTurn()` without knowing Worker boundary details.
- **HintService** (`application/hint-service.js`): Iterative deepening evaluation with cancellation tokens.
- **SettingsRepository** (`infrastructure/settings-repository.js`): localStorage persistence with migration from legacy `gameSettings` key to `deft-reversi-settings`.

### Bitboard Representation

- Position: `pos = 0..63`, where `x = pos % 8`, `y = floor(pos / 8)`
- JS uses `BigInt`; WASM boundary uses `u32 low/high` split
- Notation: `a1..h8` format in `domain/types.js`

### Event Serialization

`GameService._runExclusive()` serializes all event handling via Promise chain to prevent race conditions from concurrent user/AI moves.

### Cancellation

AI and hint computations use generation tokens. When state changes, old results are discarded without terminating the Worker.
