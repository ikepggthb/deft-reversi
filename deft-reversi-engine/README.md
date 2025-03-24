
# Deft Reversi Engine

<img src="https://img.shields.io/badge/-Rust-000000.svg?logo=rust&style=plastic">

Rustで書かれたオセロAI。

## Deft Reversi Web
Deft Reversi Engineを搭載したオセロゲームは、以下のウェブサイトでプレイできます

[ Deft Reversi](https://az.recazbowl.net/deft-reversi-web/index.html)

ソースコード: 
[ Deft Reversi (Github)](https://github.com/ikepggthb/deft_web)

## 使用した技術
 - bitboard
 - negascout探索 (PVS)
 - 置換表
 - Multi Prob Cut
 - Move ordering
   - 評価関数による浅い探索を使用したMove ordering
   - 速さ優先探索
     - 相手の合法手が少ない手から探索する
     - 終盤で使用される
   - 反復深化探索
     - キラー応手を置換表に保存し、次の探索に利用
 - 機械学習(線形回帰)を用いた評価関数
   - 学習データは、Egaroucid 自己対戦の棋譜を使用
     - https://www.egaroucid.nyanyan.dev/ja/technology/transcript/
   - 特徴量として盤面の部分パターンと、合法手数差を使用した。


## ライセンス
このプロジェクトは [GNU General Public License v3.0](LICENSE) の下で公開されています。

詳細については [LICENSE](LICENSE) ファイルを参照してください。