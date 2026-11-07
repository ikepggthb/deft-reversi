# Midgame fixed-depth comparison

This comparison uses the complete 24-position fixture in
`inputs/midgame.obf`: seeds 1–8 at 12, 24, and 36 placements. It runs each
position at depth 8 and depth 12 three times under three conditions, for 432
formal runs total. The depth-12 comparison is primary; depth 8 is reported
separately because Edax's timer is rounded to milliseconds and may report
0.000 seconds for short searches. Such readings remain included.

- `remote_deft`: frozen default Deft from remote commit `527c906`, using the
  normal `SearchMode::Depth(d)` request with `Selectivity::Full`.
- `edax_4_5_5`: deployed Edax 4.5.5 x86-64-v3.
- `edax_4_6_native`: Edax 4.6 from commit `713a434f`, built with Clang and
  `-march=native`.

Each solve runs in a fresh process pinned to CPU 1, with one thread and an empty
TT. Deft uses `eval-v4.bin`, its MPC configuration, a 256 MiB TT, and a 16 MiB
PV table. Edax uses its `eval.dat`, level 60, explicit requested depth,
`-selectivity 5` (Full at the target depth), h23, and book off. This sets the
target depth to Full, but stock Edax may use selective reduced-depth probes
and IID while iterating toward it. Deft's Full depth request uses no MPC sigma
at any iteration, so the engines' internal search procedures are not
identical. The Edax hash parameter does not imply the same memory use as Deft's
TT and PV tables.

Each result must report the requested completed depth and 100% final
selectivity. The Deft adapter checks the mode, completed depth, stop reason,
nonselective status, and the legality of its displayed PV. The runner replays
the displayed Edax PV segment and checks each shown move, including passes,
for legality. Neither engine's score is compared to another engine's score,
and neither midgame PV must reach the end of the game. Scores, best actions,
nodes, and NPS are reported as per-engine repeatability/diagnostic data; nodes
and NPS definitions differ between engines.

Run order cycles through three rotations for each position-depth-round block.
The primary metric at each depth is the sum of the 24 per-position medians over
three runs. The separate two-position depth-12 pilot is in `validation/pilot/`
and is excluded from the formal data. `environment.json`, raw rows, stdout and
stderr, per-case medians, paired ratios, and round totals record the run inputs
and outputs.

## Results

Primary metric: seconds, calculated as the sum of the 24 case medians (three
runs per case). Lower is faster.

| Requested depth | Remote Deft | Edax 4.5.5 | Edax 4.6 native |
|---:|---:|---:|---:|
| 8 | 0.0792 | 0.148 | 0.183 |
| 12 | 3.0215 | 1.296 | 1.630 |

At depth 12, Edax 4.5.5 took 57.1% less summed median time than remote Deft;
Edax 4.6 native took 46.1% less. At depth 8, Edax took longer in this fixture
set. These are results for the tested builds, model/evaluation files, host,
and positions; they do not establish a general ranking of engine versions.
The search procedures and evaluation functions differ, so timing and node
counts are not a same-work kernel comparison.

Audit: `raw-results.tsv` has 432 rows, covering 144 condition/depth/position
groups with three runs each. All 432 stdout/stderr pairs are saved. Every Deft
result passed the requested-depth, full-selectivity, and PV checks; every
Edax result reported the requested depth and its shown PV segment replayed
legally. Per-case median sums were independently recalculated from the raw
rows. Binary, model, eval-file, and frozen Deft source hashes match before
and after timing (`environment.json`).


---

2026-11-07 の整理で、解ごとの出力ログ（`results/logs/`）・ビルドのログ・計測スクリプトを入れずに追加した。今の計測には [tools/bench](../../../tools/bench/README.md) を使う。
