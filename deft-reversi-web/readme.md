# Deft Reversi Web

![image](https://github.com/ikepggthb/deft_web/assets/61868325/4c8cfa95-0b81-42c8-b24c-cb7c4855c0a1)

ブラウザで動くリバーシです。Web Worker + WASM で AI を動かし、棋譜、分析、評価グラフ、検討モードを提供します。

## 設計

実装の責務分担、データ境界、処理フローは [docs/DESIGN.md](docs/DESIGN.md) を参照してください。

## 公開中のアプリ

https://az.recazbowl.net/deft-reversi-web/index.html

## ローカル起動

`file://` 直開きでは Worker / WASM / gzip 評価データの読み込みが壊れやすいので、必ず静的 HTTP サーバーで起動してください。

例:

```bash
cd app
python3 -m http.server 8000
```

起動後:

```text
http://localhost:8000
```

テスト:

```bash
cd app
node --test ../app/test/**/*.test.mjs
```

またはリポジトリ直下で:

```bash
node --test app/test/**/*.test.mjs
```


## 難易度

レベルは `1` から `24` です。

- `3` 前後でも十分強いです
- `20` 以上はかなり重くなります
