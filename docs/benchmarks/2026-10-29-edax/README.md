# Edax and Deft comparison

This comparison runs 120 fresh-process solves in one interleaved group: 10 FFO
40–49 positions, three runs each, under four conditions:

- `remote_deft`: frozen default Deft binary from commit `527c906`.
- `merged_deft`: frozen default Deft binary from merge commit `eb76a8f`.
- `edax_4_5_5`: deployed Edax 4.5.5 x86-64-v3 binary.
- `edax_4_6_native`: Edax 4.6 built with `-march=native` from source commit
  `713a434f13b3d15fb69c61b9ba3641aed82c496c` in a `git archive` copy under
  `/tmp`; the source checkout was not modified.

Every solve uses one process pinned to CPU 1, an Intel Core Ultra 7 265K P-core.
The Deft binaries call the normal `Engine.solve(board, 60)` API with
`eval-v4.bin` and MPC, TT 256 MiB, and PV 16 MiB. Both Edax binaries use level
60, one CPU, hash parameter h23, book off, and the same `eval.dat`. Edax h23
means 2^23 main-table entries; Edax table memory is not asserted to equal the
Deft TT or PV allocation.

The Deft primary time is `SearchResult.elapsed`. Edax primary time is its own
reported search time, parsed at 1 ms resolution. Edax runs count as full-depth
only when the final row depth equals the position's empty count and no selective
trust percentage is printed. All four conditions must match the published FFO
score. The Deft adapter validates its complete PV to a terminal board and
checks its signed final score. Edax's displayed PV is retained as output but is
not assumed to be complete or validated through terminal play.

Execution order cycles through four condition rotations by case-round block;
the exact order index is in `results/raw-results.tsv`. It is approximately
balanced across positions, rather than fully balanced over 30 blocks. The
primary comparison is the sum of each condition's ten per-case medians over
three runs. Paired ratios and round totals are supplementary.

`environment.json` records source, binary, model, evaluation, and input hashes,
compiler and native build flags. `run_compare.py` stores stdout/stderr for every
run and flushes each validated result to disk before continuing. `results/`
contains raw rows, per-case medians, round totals, paired ratios, and the final
summary.

## Results

All 120 runs matched the official FFO scores. Both Edax versions reached full
depth at 100% trust on every case; the Deft PV checks reached terminal boards
with matching scores. Edax PV strings are recorded, but only the Deft PVs are
validated as complete.

| Condition | Sum of per-case medians | Change vs remote Deft |
|---|---:|---:|
| Remote Deft | 10.251324 s | baseline |
| Merged Deft | 10.194934 s | −0.550% |
| Edax 4.5.5, x86-64-v3 | 10.465 s | +2.084% |
| Edax 4.6, native | 13.669 s | +33.339% |

These results describe these four binaries on this host and workload. Edax 4.6
searched more nodes than either Deft build on most cases; per-case node medians
are recorded alongside the times. This comparison does not establish that
other Edax builds or configurations are slower. Deft and Edax use different
evaluation files and hash-table layouts, and h23 is not claimed to match
Deft's 256 MiB TT plus 16 MiB PV allocation.


---

2026-11-07 の整理で、解ごとの出力ログ（`results/logs/`）・ビルドのログ・計測スクリプトを入れずに追加した。今の計測には [tools/bench](../../../tools/bench/README.md) を使う。
