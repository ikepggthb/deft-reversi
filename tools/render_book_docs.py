#!/usr/bin/env python3
"""Render the book guide and verify local links. Requires markdown-it-py 4.2.0.

python3 tools/render_book_docs.py [--check]
The source Markdown remains authoritative. No network/CDN resources are used.
"""
from __future__ import annotations

import argparse
import html
from html.parser import HTMLParser
from pathlib import Path
import sys
from urllib.parse import unquote, urlsplit

try:
    from markdown_it import MarkdownIt
except ImportError:
    raise SystemExit("Install tools/requirements-book-docs.txt in your Python environment first.")

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs/book-v2"
ORDER = ["index", "source-review", "training", "operations", "architecture", "value-model", "levels", "loss-control",
         "scheduling", "propagation", "diagnostics", "compatibility", "design", "validation"]
STYLE = """
:root{color-scheme:light;--ink:#18313b;--muted:#52646b;--line:#d5dfe1;--accent:#096c64}
*{box-sizing:border-box}html{scroll-padding-top:1.5rem}body{margin:0;color:var(--ink);background:#f6f8f7;font:17px/1.85 system-ui,-apple-system,'Noto Sans JP',sans-serif}
a{color:var(--accent);text-underline-offset:.2em}a:focus-visible{outline:3px solid #b37600;outline-offset:3px}.skip{position:absolute;left:-9999px}.skip:focus{left:1rem;top:1rem;background:white;padding:.5rem;z-index:10}
.layout{display:grid;grid-template-columns:270px minmax(0,1fr);max-width:1500px;margin:auto}nav{padding:2rem 1.3rem;position:sticky;top:0;height:100vh;overflow:auto;border-right:1px solid var(--line)}nav .brand{font-size:1.3rem;font-weight:750;color:var(--ink)}nav ol{padding:0;list-style:none}nav li{margin:.5rem 0}nav a{display:block;font-size:.94rem;text-decoration:none;padding:.3rem .65rem;border-radius:5px}nav a[aria-current]{background:#d9eeea;font-weight:700}
main{min-width:0;padding:2.2rem clamp(1.2rem,4vw,4.5rem) 5rem;background:white}.meta{font-size:.84rem;color:var(--muted);border-bottom:1px solid var(--line);padding-bottom:1rem}.toc{background:#f1f6f4;border-left:4px solid var(--accent);padding:1rem 1.5rem}.toc ul{margin:.5rem 0;padding-left:1.3rem}.toc a{font-size:.94rem}h1{font-size:clamp(1.7rem,3vw,2.4rem);line-height:1.4;letter-spacing:.01em}h2{font-size:1.4rem;margin-top:2.4rem;border-bottom:1px solid var(--line);padding-bottom:.35rem}h3{font-size:1.15rem;margin-top:2rem}p,li{overflow-wrap:anywhere}pre{padding:1.2rem;background:#132b33;color:#edf5f3;border-radius:7px;overflow:auto;line-height:1.65;font-size:.84rem}code{font-family:ui-monospace,monospace}p code,li code,td code{background:#eef2f1;padding:.12em .3em;border-radius:3px}.table-wrap{overflow:auto}table{border-collapse:collapse;width:100%;font-size:.92rem;margin:1.3rem 0}th,td{border:1px solid var(--line);padding:.65rem .85rem;text-align:left;vertical-align:top}th{background:#e7f0ed}blockquote{margin-left:0;border-left:4px solid var(--line);padding-left:1rem;color:var(--muted)}footer{border-top:1px solid var(--line);margin-top:3rem;padding-top:1rem;color:var(--muted);font-size:.86rem}
@media(max-width:850px){.layout{display:block}nav{position:static;height:auto;padding:1rem;border-right:0;border-bottom:1px solid var(--line)}nav ol{display:flex;flex-wrap:wrap;gap:.2rem;margin-bottom:0}nav li{margin:0}nav a{font-size:.82rem}main{padding-top:1rem}}
@media print{nav,.toc,.skip{display:none}.layout{display:block}body{background:white;font-size:10pt}main{padding:0}pre{white-space:pre-wrap;border:1px solid var(--line);background:#f1f3f2;color:black}h2,h3{break-after:avoid}a{color:inherit}table{font-size:9pt}}
"""


def title(path: Path) -> str:
    return next(line[2:] for line in path.read_text(encoding="utf-8").splitlines() if line.startswith("# "))


def render(path: Path) -> str:
    md = MarkdownIt("commonmark", {"html": False}).enable("table")
    tokens = md.parse(path.read_text(encoding="utf-8"))
    toc = []
    number = 0
    for i, token in enumerate(tokens):
        if token.type == "heading_open":
            number += 1
            anchor = f"section-{number}"
            token.attrSet("id", anchor)
            if token.tag == "h2":
                toc.append(f'<li><a href="#{anchor}">{html.escape(tokens[i+1].content)}</a></li>')
        for child in token.children or []:
            if child.type == "link_open":
                href = child.attrGet("href") or ""
                parts = urlsplit(href)
                if not parts.scheme and parts.path.endswith(".md"):
                    target = (path.parent / unquote(parts.path)).resolve()
                    if target.parent == DOCS and target.stem in ORDER:
                        child.attrSet("href", target.stem + ".html" + ("#" + parts.fragment if parts.fragment else ""))
    content = md.renderer.render(tokens, md.options, {})
    content = content.replace("<table>", '<div class="table-wrap"><table>').replace("</table>", "</table></div>")
    nav = "".join(f'<li><a href="{name}.html"' + (' aria-current="page"' if path.stem == name else '')
                  + f'>{html.escape(title(DOCS / (name + ".md")))}</a></li>' for name in ORDER)
    heading = html.escape(title(path))
    return f'''<!doctype html>
<html lang="ja"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{heading} · Deft Book</title><style>{STYLE}</style></head>
<body><a class="skip" href="#content">本文へ</a><div class="layout">
<nav aria-label="Bookガイド"><div class="brand">Deft Reversi · Book</div><ol>{nav}</ol></nav>
<main id="content"><div class="meta">Book v2 / work v8 / play v4 · Markdownから生成 · 外部通信なし</div>
<aside class="toc" aria-label="このページの目次"><strong>このページ</strong><ul>{''.join(toc)}</ul></aside>
{content}<footer>正本：<a href="{path.name}">{path.name}</a> · 再生成：tools/render_book_docs.py</footer></main>
</div></body></html>
'''


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.ids = set()

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            if attrs["id"] in self.ids:
                raise ValueError("duplicate HTML id " + attrs["id"])
            self.ids.add(attrs["id"])
        if tag == "a" and "href" in attrs:
            self.links.append(attrs["href"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="reject stale HTML without writing")
    args = parser.parse_args()
    artifacts = {DOCS / (name + ".html"): render(DOCS / (name + ".md")) for name in ORDER}
    failed = []
    for path, output in artifacts.items():
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != output:
                failed.append("stale/missing: " + str(path.relative_to(ROOT)))
        else:
            path.write_text(output, encoding="utf-8")
    parsed = {}
    for path, output in artifacts.items():
        scan = Links()
        scan.feed(output)
        parsed[path] = scan
    for path, scan in parsed.items():
        for href in scan.links:
            parts = urlsplit(href)
            if parts.scheme or parts.netloc:
                continue
            target = (path.parent / unquote(parts.path)).resolve() if parts.path else path
            if not target.is_relative_to(ROOT) or not target.is_file():
                failed.append(f"broken link: {path.name} -> {href}")
            elif parts.fragment and target in parsed and unquote(parts.fragment) not in parsed[target].ids:
                failed.append(f"broken anchor: {path.name} -> {href}")
    if failed:
        print("\n".join(failed), file=sys.stderr)
        return 1
    print(f"{'Checked' if args.check else 'Rendered'} {len(artifacts)} pages; local links and anchors valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
