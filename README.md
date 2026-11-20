# shinytauri

Tauri 2 のウィンドウで Rhino（Shiny）アプリを実行するプロジェクトです。最終的な配布先は、R のインストールやインターネット接続のない Windows PC です。

## 開発用アプリの起動

リポジトリのルートで `nix develop` に入ります。開発シェルと mise タスクでは `RIG_MODE=user` を設定しています。初回はネットワークに接続し、次を実行してください。

```sh
mise run setup:r
cargo tauri dev
```

`cargo tauri dev` は Rhino を `127.0.0.1:3838` で起動し、Tauri のウィンドウに表示します。「1 を足す」を押すと、R がカウントを更新します。ブラウザーだけで開発するときは、代わりに `rhino/` で `rig run dev.R` を実行し、同じアドレスを開いてください。どちらも同じポートを使うので、切り替えるときは先に起動した方を終了してください。

Shiny は起動時に mirai のローカルデーモンを4つ立ち上げます。全セッションでこの4つを共有し、アプリの終了時にまとめて停止します。時間のかかる計算は、サーバーモジュールの `shiny::ExtendedTask` から `mirai::mirai()` に渡せます。[mirai の Shiny 統合ガイド](https://mirai.r-lib.org/articles/shiny.html)

デモの「サイコロを振る」は、選んだ回数だけ2個のサイコロを振り、出目の合計の実測値を理論値と比較します。計算中に「1 を足す」を試せるよう、デーモン内に2秒の待ち時間を設けています。モジュールの実装は `rhino/app/view/dice.R` にあります。

## ソースと依存関係

- `src-tauri/` に Rust のエントリーポイントと Tauri 設定を置きます。
- `rhino/` が Rhino と rig のプロジェクトルートです。`app.R` が入口で、画面とサーバーの実装は `app/main.R`、画像などの静的ファイルは `app/static/` に置きます。
- R の直接依存は `rhino/rproj.toml` で宣言します。`rhino/` で `rig proj add パッケージ名` を実行し、更新されたマニフェストと `rproj.lock` を一緒にコミットします。マニフェストを手で編集した場合は `rig proj lock` でロックを更新します。[rig のプロジェクトガイド](https://rig.r-lib.org/proj-guide.html)

ロックファイルには R 4.6.2 の x86_64 Linux 用と x64 Windows 用の依存関係を記録しています。`mise run setup:r` は `rhino/` で `rig proj sync --locked` を実行します。rig はロックされた R がユーザー領域に未導入ならインストールし、パッケージを `rhino/.rvenv/lib/` に復元します。ロックファイルの更新後も同じタスクを使えます。

## Windows 配布に向けて

現段階で Rhino を起動するのは開発コマンドです。`cargo tauri build` で作るアプリには `ui/index.html` の起動案内が入ります。配布版では、Windows 用にロックから復元したライブラリ、ポータブル R、Rhino のソースを同梱し、Tauri からローカルの R プロセスを起動・終了する予定です。開発用の `.rvenv/` はマシン固有の生成物です。

WebView2 が未導入の PC にもオフラインでインストールできるよう、配布版では WebView2 のオフラインインストーラーも同梱する予定です。[Tauri の Windows 配布ガイド](https://v2.tauri.app/distribute/windows-installer/#offline-installer)

## 開発時のチェック

リポジトリのルートで `nix develop` を実行すると Nushell が起動します。mise のタスクも Nushell で実行されます。

初回はリポジトリのルートで `nix develop -c mise run setup` を実行します。
このクローンの pre-commit フックが有効になり、`git merge` は fast-forward できる場合もマージコミットを作成します。続けて `setup:r` が R 環境を復元します。
`setup` の最後に Git の設定値とフックの実行権限を確認し、各項目の結果を表示します。開発シェルでは `mise run setup:check` でいつでも再確認できます。

開発シェルでは `mise run QA` で Nix・TOML・R・Nushell のフォーマットと lint を確認できます。
フォーマットだけなら `mise run format`、lint だけなら `mise run lint` で、言語別のチェックを並列実行できます。
コミット時も同じタスクが作業ツリーをチェックし、問題があれば該当ファイルを報告してコミットを止めます。

`mise run QA-fix` は Nix・R・Nushell の lint 違反のうち自動修正できるものを直し、Nix・TOML・R・Nushell を整形します。
すべての修正が完了してから `QA` を実行し、違反が残っていれば失敗します。

### テスト

開発シェルにはブラウザーテスト用の Chromium が含まれます。`mise run setup:r` で testthat と shinytest2 を含む開発用パッケージを復元してください。

| コマンド | 実行内容 |
| --- | --- |
| `mise run test` | Rhino の単体テスト、shinytest2 のブラウザーテスト、Tauri の nextest を並列実行 |
| `mise run test-full` | 同じ3系統で重いテストも実行 |
| `mise run QA-ci` | `QA` と `test` |
| `mise run QA-full` | `QA` と `test-full` |

通常のブラウザーテストはカウンターを操作します。重いテストでは10万回のサイコロ集計を起動し、実行中の応答と完了後の結果を確認します。`test-full` は `SHINYTAURI_HEAVY_TESTS=1` を設定して `test` を呼び出します。個別に試す場合は、たとえば `env SHINYTAURI_HEAVY_TESTS=1 mise run test:shinytest2` を実行します。

R のテストは `rhino/tests/testthat/` に置きます。`test-shinytest2*.R` を `test:shinytest2`、それ以外を `test:testthat` が実行します。重いケースには `testthat::skip_if_not(Sys.getenv("SHINYTAURI_HEAVY_TESTS") == "1")` を付けます。Rust では重いテストに `#[ignore]` を付けると、同じ環境変数で `test:tauri` が実行対象に加えます。
