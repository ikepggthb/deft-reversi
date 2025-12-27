# WebAssembly Module (`wasm`)

This directory contains the Rust source code for the Reversi engine, which is compiled into a WebAssembly (Wasm) module for use in the web application.

## Building the Module

The Wasm module is built using `wasm-pack`. For this project, the output needs to be placed in the `../app/pkg` directory so the web application can use it.

From within this `wasm/` directory, run the following command:

```shell
wasm-pack build --target web --out-dir ../app/pkg
```

-   `--target web`: Ensures the output is compatible with web browsers.
-   `--out-dir ../app/pkg`: Places the compiled files into the web application's `pkg` directory.

---
# 日本語

# WebAssembly モジュール (`wasm`)

このディレクトリには、Webアプリケーションで使用するためにWebAssembly（Wasm）モジュールへコンパイルされる、Rustで書かれたリバーシエンジンのソースコードが含まれています。

## ビルド方法

Wasmモジュールのビルドには `wasm-pack` を使用します。このプロジェクトでは、Webアプリケーションがモジュールを利用できるよう、出力先を `../app/pkg` ディレクトリに指定する必要があります。

この `wasm/` ディレクトリ内で、以下のコマンドを実行してください。

```shell
wasm-pack build --target web --out-dir ../app/pkg
```

-   `--target web`: 出力形式をWebブラウザ互換にします。
-   `--out-dir ../app/pkg`: コンパイルされたファイルをWebアプリケーションの `pkg` ディレクトリに配置します。
