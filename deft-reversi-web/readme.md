# Deft Reversi Web
![image](https://github.com/ikepggthb/deft_web/assets/61868325/4c8cfa95-0b81-42c8-b24c-cb7c4855c0a1)

## AIと遊ぶ
https://az.recazbowl.net/deft-reversi-web/index.html

## AI部分のソースコード
[Deft Reversi Engine (Github)](https://github.com/ikepggthb/deft-reversi-engine)

## 難易度について

レベルは、0~24まであります。

レベル3程度でも、ほとんどの人が勝てないのではないかと思います。

レベル20以上は、重いので推奨しないです。

# Note

ビルド方法

- Rustと、wasm-packが必要です。

```
git clone https://github.com/ikepggthb/deft_web.git
cd deft_web
git clone https://github.com/ikepggthb/deft-reversi-engine.git

cargo install wasm-pack
wasm-pack build --target web
```
