# Deft Reversi ロゴキット

マーク：正方形のタイルを左右半分に分け、左は黒地に白い反転D（ᗡ = Reverse）、右は白地に黒いD（Deft）を、背中合わせに置いた形です。

## ファイル

| 用途 | ファイル |
|---|---|
| マーク単体 | `svg/deft-mark-{light,dark,mono-black,mono-white}.svg` / `png/mark/*-{256,512,1024}.png` |
| 横組みロゴ | `svg/deft-logo-horizontal-*.svg` / `png/logo/deft-logo-horizontal-*.png`（高さ256px） |
| 縦組みロゴ | `svg/deft-logo-stacked-*.svg` / `png/logo/deft-logo-stacked-*.png`（高さ512px） |
| アプリアイコン | `svg/deft-app-icon.svg` / `png/icon/deft-app-icon-{1024,512,192}.png`（全面に配置。角はOS側で丸められます） |
| ファビコン | `favicon.ico`（16/32/48）、`svg/favicon.svg`、`png/icon/favicon-*.png`、`png/icon/apple-touch-icon.png` |
| OGP画像 | `png/og-image.png`（1200×630） |

- `light` = 明るい背景用、`dark` = 暗い背景用、`mono-*` = 1色版（白い部分を透明にしているので、背景の色がそのまま見えます。色のついた背景や写真の上で使えます）
- SVGの文字はアウトライン化済みです。フォントのインストールは要りません。
- 16pxのファビコンは、枠線が細すぎてぼやけないように、枠線だけを太くしています。

## 色

| 名前 | HEX |
|---|---|
| Ink | `#111316` |
| Paper | `#FFFFFF`（暗い背景では `#F4F2EC`） |

## 書体

Space Grotesk（SIL OFL 1.1）。"Deft"はBold、"Reversi"はRegular、字間は-0.02em。

## 使い方の目安

- 余白：マークの高さの1/4以上を空けてください。
- 最小サイズ：マーク単体は16px、横組みロゴは高さ20px。
- 角を丸めたり、傾けたり、左右を反転させたりしないでください（左右の意味が入れ替わってしまいます）。

## HTML

```html
<link rel="icon" href="/favicon.ico" sizes="any">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<meta property="og:image" content="/og-image.png">
```

README（GitHub）用：

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="svg/deft-logo-horizontal-dark.svg">
  <img src="svg/deft-logo-horizontal-light.svg" alt="Deft Reversi" height="48">
</picture>
```
