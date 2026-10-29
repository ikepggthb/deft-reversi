/* tslint:disable */
/* eslint-disable */

export class WasmEngine {
    free(): void;
    [Symbol.dispose](): void;
    clearTt(): void;
    /**
     * Model bytes from fetch().arrayBuffer(); ttMiB must be 1..=256.
     */
    constructor(model_bytes: Uint8Array, tt_mib: number);
    search(position: WasmPosition, request: any): any;
    setModel(bytes: Uint8Array): void;
}

/**
 * A position including the side to move. Moves return a new position.
 */
export class WasmPosition {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * A1..H8 order: 0 = empty, 1 = black, 2 = white.
     */
    cells(): Uint8Array;
    /**
     * Construct a side-to-move-relative position from bitboards.
     */
    static fromBits(player: bigint, opponent: bigint): WasmPosition;
    static fromString(value: string): WasmPosition;
    isTerminal(): boolean;
    legalMoves(): string[];
    mustPass(): boolean;
    constructor();
    play(action: string): WasmPosition;
    sideToMove(): string;
    toString(): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_wasmengine_free: (a: number, b: number) => void;
    readonly __wbg_wasmposition_free: (a: number, b: number) => void;
    readonly wasmengine_clearTt: (a: number) => [number, number];
    readonly wasmengine_new: (a: number, b: number, c: number) => [number, number, number];
    readonly wasmengine_search: (a: number, b: number, c: any) => [number, number, number];
    readonly wasmengine_setModel: (a: number, b: number, c: number) => [number, number];
    readonly wasmposition_cells: (a: number) => [number, number];
    readonly wasmposition_fromBits: (a: bigint, b: bigint) => [number, number, number];
    readonly wasmposition_fromString: (a: number, b: number) => [number, number, number];
    readonly wasmposition_isTerminal: (a: number) => number;
    readonly wasmposition_legalMoves: (a: number) => [number, number];
    readonly wasmposition_mustPass: (a: number) => number;
    readonly wasmposition_new: () => number;
    readonly wasmposition_play: (a: number, b: number, c: number) => [number, number, number];
    readonly wasmposition_sideToMove: (a: number) => [number, number];
    readonly wasmposition_toString: (a: number) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_drop_slice: (a: number, b: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
