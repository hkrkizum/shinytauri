# shinytauri

shinytauri は R の Shiny アプリを Windows x64 用のインストーラーにまとめる CLI である。既存のプロジェクトで実行すると、アプリ、ポータブル R、実行に使う R パッケージを同梱した `*-setup.exe` ができる。配布先では Tauri のウィンドウにアプリが開く。

[利用マニュアル](https://hkrkizum.github.io/shinytauri/) · [リリース](https://github.com/hkrkizum/shinytauri/releases) · [開発に参加する](docs/development.qmd)

## インストール

ビルド機に合う x86_64 用のアーカイブを [GitHub Releases](https://github.com/hkrkizum/shinytauri/releases) から取得する。展開した `shinytauri`（Linux）または `shinytauri.exe`（Windows）を PATH の通ったディレクトリに置く。ソースからインストールする場合はリポジトリのルートで `cargo install --locked --path cli` を実行する。

| ビルド機 | 必要な環境 |
| --- | --- |
| Linux x86_64 | Git、flakes と `nix-command` を有効にした Nix |
| Windows x64 | Visual Studio Build Tools の C++ ツールと Windows SDK、Rust の MSVC ツールチェーン、rig、Tauri CLI |

ビルドにはインターネット接続が必要である。配布先は WebView2 Runtime を導入した Windows 10/11 x64 で、R を別途インストールする必要はない。[OS 別の導入手順](docs/installation.qmd)

## 最初のビルド

次は `app.R` で Shiny だけを使うアプリの例である。初回に配布設定を作り、同梱の最小ロックを使ってビルドする。

```sh
cd sales-dashboard
shinytauri init . --id jp.example.sales-dashboard
shinytauri build .
```

`dist/` にできた `*-setup.exe` が配布物である。画面や計算処理を変更したら、同じディレクトリで `shinytauri build .` を実行する。配布ファイルを追加したときは `shinytauri.toml` の `files` も更新する。

`DT` などを使う場合は `--package` で依存を宣言する。`renv.lock` や `DESCRIPTION` を使っている場合は、それらをもとに `shinytauri lock .` で Windows 用ロックを作成できる。既存の rig プロジェクトは Windows 用の `rproj.lock` を再利用できる。[アプリの依存パッケージ](docs/dependencies.qmd)

依存定義のないアプリで配布用 R を固定するには、初期化時に `--r-version 4.5.3` を指定し、`shinytauri lock .` と `shinytauri build .` を実行する。既存の `renv.lock` や rig の Windows 用ロックが R を指定している場合は、そのバージョンを使える。[同梱する R の選び方](docs/dependencies.qmd)

動くアプリのコードから試すには[はじめてのビルド](docs/index.qmd)、同梱するファイルや製品名を変えるには[配布設定のリファレンス](docs/configuration.qmd)を参照する。

## ライセンス

[MIT License](LICENSE)
