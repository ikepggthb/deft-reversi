"""ベンチマーク用スクリプトが共有するパス解決、エンジン設定、局面ファイル読み込み処理。

エンジン本体・評価モデル・データ類は Git 管理外の `local/` ディレクトリに配置する
（詳細は local/README.md を参照）。既定の配置場所は DEFAULTS に定義しており、
`local/bench.toml` を置くと任意の項目（リポジトリルートからの相対パス、または絶対パス）を上書きできる。
"""
import io
import csv
import hashlib
import os
import re
import subprocess
import sys
import tomllib
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
POSITIONS = Path(__file__).resolve().parent / "positions"
DEFAULTS = {
    # RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native でビルドしたバイナリを使う。
    "solve": "target-native/release/deft-reversi-cli",
    "gtp": "target-native/release/deft-reversi-cli",
    "model": "local/release/model-v4/deft-eval-v4.dpr2",
    # 配布版の Edax 4.5.6。bin/ の隣にある data/eval.dat を参照する。
    "edax": "local/bench-engines/edax-4.5.6/bin/lEdax-x86-64-v3",
    # 公式の clang++ コマンドでビルドした Egaroucid for Console 7.8.2。
    "egaroucid": "local/bench-engines/Egaroucid-console_v7.8.2/bin/Egaroucid_for_Console.out",
    # XOT の開局リスト（large）。リポジトリには同梱せず、初回に配布元から取得する（fetch_xot）。
    "xot": "local/xot/openingslarge.txt",
}

# XOT（Matthias Berg 氏）の配布元と、取得したファイルの SHA-256。
XOT_URL = "https://berg.earthlingz.de/xot/downloads/openingslarge.txt"
XOT_SHA256 = "ccbeb0ed7b7ae23de20013a3992b9a6ede77610bbb5cacda82ccbaedaab72530"


def paths():
    """local/bench.toml の上書きを反映した各ツール・モデル・エンジンの絶対パス辞書を返す。"""
    values = dict(DEFAULTS)
    config = REPO / "local/bench.toml"
    if config.exists():
        values.update(tomllib.loads(config.read_text()))
    return {k: Path(v) if Path(v).is_absolute() else REPO / v for k, v in values.items()}


def fetch_xot(path, url=XOT_URL):
    """XOT の開局リストを path に用意して返す。なければ配布元から取得し、SHA-256 を確かめてから保存する。"""
    path = Path(path)
    if path.exists():
        return path
    print(f"XOT の開局リストを取得します: {url}", file=sys.stderr)
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != XOT_SHA256:
        sys.exit(f"XOT の開局リストの SHA-256 が一致しません: {digest}（期待値 {XOT_SHA256}）")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


def read_answers(path):
    """正解付き .obf ファイル（`盤面 手番; A2:+38; ...` 形式）を読み込み、
    `(局面文字列, 最善手スコア)` のリストを返す（スコア記載がなければ None）。
    """
    out = []
    for line in Path(path).read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        pos, _, rest = line.partition(";")
        scores = [int(s) for s in re.findall(r":\s*([+-]?\d+)", rest)]
        out.append((pos.strip(), max(scores) if scores else None))
    return out


def empties(position):
    """局面文字列から空きマス（`-`）の個数を数えて返す。"""
    return position.split()[0].count("-")


class Deft:
    """Deft エンジンの構成を表す。`NAME` または `NAME:key=value,...` 形式で指定する。

    指定できるキー:
      solve: CLI バイナリのパス
      model: 評価モデルファイルのパス
      tt:    置換表のサイズ（MiB、既定は 256）
      大文字のみのキー: サブプロセスに渡す環境変数（一時的な実験用スイッチなどに使う）
    """

    kind = "deft"

    def __init__(self, spec, tools):
        self.name, _, settings = spec.partition(":")
        self.solve, self.model, self.tt, self.env = tools["solve"], tools["model"], 256, {}
        self.reported_versions = set()
        for item in filter(None, settings.split(",")):
            key, _, value = item.partition("=")
            if key == "solve":
                self.solve = Path(value)
            elif key == "model":
                self.model = Path(value)
            elif key == "tt":
                self.tt = int(value)
            elif key.isupper():
                self.env[key] = value
            else:
                raise SystemExit(f"unknown setting {key!r} in {spec!r}")

    def solve_file(self, case, goal, threads, cpus):
        """指定した局面ファイルを `deft-reversi-cli solve` で探索し、TSV 出力を辞書のリストで返す。"""
        goal_args = ["--exact"] if goal == "exact" else (["--level", goal[5:]] if goal.startswith("level") else ["--depth", goal])
        cmd = ["taskset", "-c", cpus, str(self.solve), "solve", str(Path(case).resolve()),
               "--model", str(self.model), *goal_args, "--threads", str(threads), "--tt-mib", str(self.tt), "--tsv"]
        result = subprocess.run(cmd, capture_output=True, text=True, check=True, env={**os.environ, **self.env})
        for line in result.stderr.splitlines():
            if line.startswith("Versions: ") and line not in self.reported_versions:
                print(f"{self.name}: {line}", file=sys.stderr, flush=True)
                self.reported_versions.add(line)
        return list(csv.DictReader(io.StringIO(result.stdout), delimiter="\t"))


def engine(spec, tools):
    """指定文字列（`edax`、`egaroucid`、または Deft 構成文字列）に応じたエンジンラッパーを返す。"""
    if spec == "edax":
        return Edax(tools)
    if spec == "egaroucid":
        return Egaroucid(tools)
    return Deft(spec, tools)


class Edax:
    """Edax 4.5.6 を外部プロセスとして呼び出し、完全読み結果を解析するラッパー。"""

    kind = name = "edax"

    def __init__(self, tools):
        self.binary = tools["edax"]

    def exact(self, case, threads, cpus, hash_bits=23):
        """Book なしで完全読みを実行し、`(深さ, 石差スコア, 秒数, ノード数)` を返す。"""
        cmd = ["taskset", "-c", cpus, str(self.binary), "-solve", str(case), "-n", str(threads), "-l", "60",
               "-h", str(hash_bits), "-eval-file", "data/eval.dat", "-book-usage", "off"]
        out = subprocess.run(cmd, cwd=self.binary.parent, capture_output=True, text=True, check=True).stdout
        d, s, mi, se, n = re.findall(r"^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)", out, re.M)[0]
        return int(d), int(s), int(mi) * 60 + float(se), int(n)


class Egaroucid:
    """Egaroucid for Console 7.8.2 を外部プロセスとして呼び出し、完全読み結果を解析するラッパー。"""

    kind = name = "egaroucid"

    def __init__(self, tools):
        self.binary = tools["egaroucid"]

    def exact(self, case, threads, cpus, hash_bits=23):
        """Book なしで完全読みを実行し、`(深さ, 石差スコア, 秒数, ノード数)` を返す。"""
        cmd = ["taskset", "-c", cpus, str(self.binary), "-t", str(threads), "-l", "60", "-nobook",
               "-hash", str(hash_bits), "-s", str(case)]
        out = subprocess.run(cmd, cwd=self.binary.parent, capture_output=True, text=True, check=True).stdout
        d, s, h, mi, se, n = re.findall(
            r"^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)", out, re.M)[0]
        return int(d), int(s), int(h) * 3600 + int(mi) * 60 + float(se), int(n)
