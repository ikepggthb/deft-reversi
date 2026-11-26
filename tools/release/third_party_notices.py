#!/usr/bin/env python3
"""リリースのバイナリに含まれる依存クレートのライセンス一覧（THIRD-PARTY-NOTICES.txt）を作る。

使い方:
  python3 tools/release/third_party_notices.py [--target TRIPLE] [--package NAME ...] [--out FILE]

Cargo.lock の依存関係（`cargo metadata --locked`）から、配布するパッケージ（既定はデスクトップ版と
CLI）が通常の依存としてリンクするクレートを集め、各クレートのソースにあるライセンスファイルの本文を載せる。
同じ本文は 1 回だけ載せ、適用するクレートを列挙する。ライセンスファイルを同梱していない
クレートは、MIT を選べるものなら Cargo の authors から著作権表示を作って MIT の本文を、
Apache-2.0 なら他のクレートが同梱している Apache-2.0 の標準の本文を載せる。
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ROOTS = ("deft_reversi_desktop", "deft-reversi-cli")  # --package の既定値
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense)([-_.].*)?$", re.I)
MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""


def metadata(target):
    command = ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target]
    # Windows の Python の既定の文字コード（cp1252 など）ではなく、cargo が出す UTF-8 で読む。
    result = subprocess.run(command, cwd=REPO, check=True, capture_output=True, encoding="utf-8")
    return json.loads(result.stdout)


def shipped(meta, roots):
    """roots から通常の依存でたどれる、ワークスペース外のパッケージを返す。

    dev・build の依存と、コンパイル時にだけ動く proc-macro（とその依存）はバイナリに入らないので除く。
    """
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    workspace = set(meta["workspace_members"])
    stack = [i for i in workspace if packages[i]["name"] in roots]
    if len(stack) != len(set(roots)):
        sys.exit(f"ワークスペースにないパッケージがあります: {', '.join(roots)}")
    seen = set()
    while stack:
        node = stack.pop()
        if node in seen or any("proc-macro" in t["kind"] for t in packages[node]["targets"]):
            continue
        seen.add(node)
        for dep in nodes[node]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    return sorted((packages[i] for i in seen - workspace), key=lambda p: (p["name"], p["version"]))


def license_texts(package, apache):
    """パッケージのライセンス本文の一覧を返す。ファイルがなければ MIT か Apache-2.0 の本文を使う。"""
    root = Path(package["manifest_path"]).parent
    files = sorted(f for f in root.iterdir() if LICENSE_FILE.match(f.name) and f.is_file())
    if package.get("license_file"):
        files.append(root / package["license_file"])
    texts = []
    for f in dict.fromkeys(files):
        text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()
        if text:
            texts.append(text)
    if texts:
        return texts
    licenses = re.split(r"[\s/()]+", package.get("license") or "")
    if "MIT" in licenses:
        authors = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in package.get("authors") or []) or "the authors"
        return [f"MIT License\n\nCopyright (c) {authors}\n\n{MIT}".strip()]
    if "Apache-2.0" in licenses and apache:
        return [apache]
    sys.exit(f"{package['name']} {package['version']}: ライセンスファイルがなく、MIT も Apache-2.0 も選べません")


def standard_apache(packages):
    """依存のどれかが同梱している、Apache-2.0 の標準の本文（著作権表示を含まないもの）を返す。"""
    for package in packages:
        for f in sorted(Path(package["manifest_path"]).parent.glob("LICENSE-APACHE*")):
            text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()
            if text.startswith("Apache License") and text.rstrip().endswith("limitations under the License."):
                return text
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", default="x86_64-pc-windows-msvc", help="配布するバイナリのターゲット")
    parser.add_argument("--package", action="append", help=f"配布するパッケージ（複数可、既定は {' と '.join(ROOTS)}）")
    parser.add_argument("--out", type=Path, default=REPO / "target" / "THIRD-PARTY-NOTICES.txt")
    args = parser.parse_args()

    packages = shipped(metadata(args.target), args.package or ROOTS)
    apache = standard_apache(packages)
    groups = {}
    for package in packages:
        label = f"{package['name']} {package['version']}"
        for text in license_texts(package, apache):
            groups.setdefault(text, []).append(label)

    out = [
        "Third-party software in Deft Reversi",
        "",
        f"Deft Reversi ({args.target}) includes the following Rust crates. Each is distributed",
        "under its own license, reproduced below. Deft Reversi itself is licensed under",
        "GPL-3.0-or-later (see COPYING).",
        "",
        "Crate | Version | License | Repository",
    ]
    for p in packages:
        out.append(f"{p['name']} | {p['version']} | {p.get('license') or '-'} | {p.get('repository') or '-'}")
    for text, labels in groups.items():
        out += ["", "=" * 78, "Applies to: " + ", ".join(labels), "=" * 78, "", text]
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"{args.out}: {len(packages)} crates, {len(groups)} license texts")


if __name__ == "__main__":
    main()
