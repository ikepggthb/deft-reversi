/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const __wbg_wasmengine_free: (a: number, b: number) => void;
export const __wbg_wasmposition_free: (a: number, b: number) => void;
export const wasmengine_clearTt: (a: number) => [number, number];
export const wasmengine_new: (a: number, b: number, c: number) => [number, number, number];
export const wasmengine_search: (a: number, b: number, c: any) => [number, number, number];
export const wasmengine_setModel: (a: number, b: number, c: number) => [number, number];
export const wasmposition_cells: (a: number) => [number, number];
export const wasmposition_fromBits: (a: bigint, b: bigint) => [number, number, number];
export const wasmposition_fromString: (a: number, b: number) => [number, number, number];
export const wasmposition_isTerminal: (a: number) => number;
export const wasmposition_legalMoves: (a: number) => [number, number];
export const wasmposition_mustPass: (a: number) => number;
export const wasmposition_new: () => number;
export const wasmposition_play: (a: number, b: number, c: number) => [number, number, number];
export const wasmposition_sideToMove: (a: number) => [number, number];
export const wasmposition_toString: (a: number) => [number, number];
export const __wbindgen_malloc: (a: number, b: number) => number;
export const __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
export const __wbindgen_exn_store: (a: number) => void;
export const __externref_table_alloc: () => number;
export const __wbindgen_externrefs: WebAssembly.Table;
export const __externref_table_dealloc: (a: number) => void;
export const __wbindgen_free: (a: number, b: number, c: number) => void;
export const __externref_drop_slice: (a: number, b: number) => void;
export const __wbindgen_start: () => void;
