#!/usr/bin/env python3
"""展開した CLI のアーカイブが、そのままで動くことを確かめる。

使い方:
  python3 tools/release/smoke_cli.py DIR

DIR は展開したフォルダ（実行ファイルと deft-eval-v4.dpr2 がある場所）。--model も DEFT_MODEL も
渡さずに、perft 8 の末端数と、FFO 1〜19 の完全読みの石差を正解と照らし合わせる。
"""
import csv
import io
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PROBLEMS = REPO / "deft-reversi-cli/problem/fforum-1-19.obf"
FILES = ("COPYING", "README.md", "THIRD-PARTY-NOTICES.txt", "deft-eval-v4.dpr2")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    folder = Path(sys.argv[1]).resolve()
    exe = folder / ("deft-reversi-cli.exe" if os.name == "nt" else "deft-reversi-cli")
    missing = [name for name in (exe.name, *FILES) if not (folder / name).is_file()]
    if missing:
        sys.exit(f"アーカイブにないファイルがあります: {', '.join(missing)}")
    env = {k: v for k, v in os.environ.items() if k != "DEFT_MODEL"}

    def run(*args):
        result = subprocess.run([str(exe), *args], cwd=REPO, env=env, capture_output=True,
                                encoding="utf-8", errors="replace")
        if result.returncode != 0:
            sys.exit(f"{' '.join(args)} が失敗しました:\n{result.stderr}")
        return result.stdout

    if not re.search(r"^\s*8 \|\s*390216 \|", run("perft", "8"), re.M):
        sys.exit("perft 8 の末端数が 390216 ではありません")
    answers = [max(int(s) for s in re.findall(r":([+-]\d+)", line))
               for line in PROBLEMS.read_text(encoding="utf-8").splitlines() if line.strip()]
    rows = list(csv.DictReader(io.StringIO(run("solve", str(PROBLEMS), "--exact", "--tsv")), delimiter="\t"))
    wrong = [(i + 1, a, r["score"]) for i, (a, r) in enumerate(zip(answers, rows)) if int(r["score"]) != a]
    if len(rows) != len(answers) or wrong:
        sys.exit(f"FFO 1〜19 の石差が正解と一致しません（問題, 正解, 結果）: {wrong}")
    print(f"{folder.name}: perft 8 と FFO 1〜19 の完全読み {len(rows)} 問が正解と一致しました")


if __name__ == "__main__":
    main()
