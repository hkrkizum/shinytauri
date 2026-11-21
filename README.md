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

## 集計を Quarto ダッシュボードとして保存する

1. 試行回数を選び、「サイコロを振る」を押します。
2. 集計が表示されたら「レポートを作成」を押します。この時点のカウンター値と計算済みの出目別件数を使います。試行回数は集計に使った実際の回数です。
3. 作成が終わると「ダウンロード」が表示されます。クリックして `dice-dashboard.html` を保存します。ファイルを単独で別の PC にコピーしても、ブラウザーで集計表と割合を確認できます。

レンダリングはセッションごとの `shiny::ExtendedTask` から mirai デーモンに渡します。作成中もアプリを操作でき、その後の入力や集計は、作成中のレポートに影響しません。新しい値で保存するには、もう一度「レポートを作成」を押します。

レイアウトは `rhino/app/report/dashboard.qmd`、値の受け渡しと HTML の後処理は `rhino/app/report/render.R` にあります。Quarto の `embed-resources: true` で生成した HTML を R で読み、`data:` URL のスタイルシート・CSS の入れ子の `@import`・スクリプトを復号し、HTML 内の `<style>` と `<script>` に展開します。画像とフォントは埋め込みのままです。プレビューサービス側でインライン CSS や JavaScript も制限される場合は、保存した HTML をローカルブラウザーで開いてください。

## ソースと依存関係

- `src-tauri/` に Rust のエントリーポイントと Tauri 設定を置きます。
- `rhino/` が Rhino と rig のプロジェクトルートです。`app.R` が入口で、画面とサーバーの実装は `app/main.R`、画像などの静的ファイルは `app/static/` に置きます。
- R の直接依存は `rhino/rproj.toml` で宣言します。`rhino/` で `rig proj add パッケージ名` を実行し、更新されたマニフェストと `rproj.lock` を一緒にコミットします。マニフェストを手で編集した場合は `rig proj lock` でロックを更新します。[rig のプロジェクトガイド](https://rig.r-lib.org/proj-guide.html)

ロックファイルには R 4.6.2 の x86_64 Linux 用と x64 Windows 用の依存関係を記録しています。`mise run setup:r` は `rhino/` で `rig proj sync --locked` を実行します。rig はロックされた R がユーザー領域に未導入ならインストールし、パッケージを `rhino/.rvenv/lib/` に復元します。ロックファイルの更新後も同じタスクを使えます。

## Windows x64 用オフラインインストーラー

配布物は NSIS 形式の `shinytauri_*_x64-setup.exe` 1 ファイルです。導入先は **Microsoft Edge WebView2 Runtime がインストール済みの Windows 10/11 x64** を想定しています。受け取った PC はオフラインのままインストーラーを実行し、スタートメニューから起動できます。R と Quarto は同梱されており、rig や開発ツールの導入も不要です。インストーラーの `webviewInstallMode` は `skip` で、既存の WebView2 を利用します。

### ビルド

ネットに接続できるビルド機で、リポジトリのルートから実行します。x86_64 Linux では、クロスコンパイル用の Nix シェルに Rust の Windows ターゲット、MinGW-w64、NSIS、R とビルド用 R パッケージ、mise が含まれます。

```sh
nix develop .#windows -c mise trust
nix develop .#windows -c mise run build:windows
```

Windows x64 でネイティブビルドする場合は、Visual Studio Build Tools の「C++ によるデスクトップ開発」と Windows SDK、rustup（x64 MSVC）、R 4.6.2（`Rscript` に PATH を通す）、Nushell、mise を準備します。PowerShell で次を実行してください。

```powershell
Rscript --vanilla -e "install.packages(c('RcppTOML', 'jsonlite'), repos='https://cloud.r-project.org')"
cargo install tauri-cli --version 2.12.6 --locked
mise trust
mise run build:windows
```

成果物は `src-tauri/target/<target>/release/bundle/nsis/` に出力されます。`<target>` は Linux ビルドで `x86_64-pc-windows-gnu`、Windows ビルドで `x86_64-pc-windows-msvc` です。

### 同梱内容と起動の仕組み

[shinyelectron の bundled 方式](https://github.com/coatless-rpkg/shinyelectron)を参考に、ポータブル R とアプリ専用ライブラリを Tauri のリソースとして配置します。ビルドタスクは `rhino/rproj.lock` の Windows x64 ターゲットを読み、対応する [portable-r-windows](https://github.com/portable-r/portable-r-windows/releases) の ZIP の SHA-256 を照合します。続いて `main` グループの Windows バイナリをロックされた URL から取得し、パッケージ名とバージョンを照合して、Rhino のコード・設定と一緒に格納します。R を更新するときはロックの更新と合わせて `scripts/build-windows.R` の公式配布物の SHA-256 も更新してください。

同じビルドタスクが Quarto CLI 1.11.9 の公式 Windows ZIP を取得し、固定した SHA-256 を照合して `offline/quarto/` に展開します。配布版の Tauri は同梱 R を直接起動し、同梱 Quarto のパスを `SHINYTAURI_QUARTO` で mirai デーモンに引き継ぎます。開発時は `nix develop` が用意する `quarto` を使用します。[Quarto CLI の公式リリース](https://github.com/quarto-dev/quarto-cli/releases/tag/v1.11.9)

Shiny がループバックの空きポートで待ち受けてから、その URL をウィンドウに表示します。デスクトップアプリを閉じると Windows のジョブオブジェクトが R と mirai の子プロセスを終了します。起動に失敗した場合はウィンドウタイトルに表示されるログを確認してください。

### 配布前のオフライン確認

1. WebView2 Runtime 導入済みで、R と Quarto が入っていない Windows x64 のテスト環境を用意します。
2. インストーラーをコピーしてネットワークを切断し、インストールから初回起動まで実行します。
3. 「1 を足す」でカウントが変わることと、「サイコロを振る」で計算結果が表示されることを確認します。
4. 「レポートを作成」を押し、作成中にカウンターを変更します。「ダウンロード」で HTML を保存し、ブラウザーで開いて作成時点のカウンター値・試行回数と照合します。
5. ウィンドウを閉じ、同梱 R のプロセスが終了することと、オフラインのまま再起動できることを確認します。

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

通常のブラウザーテストは、カウンターの操作と、レポート作成中の入力変更・ダウンロード内容を確認します。重いテストでは10万回のサイコロ集計を起動し、実行中の応答と完了後の結果を確認します。`test-full` は `SHINYTAURI_HEAVY_TESTS=1` を設定して `test` を呼び出します。個別に試す場合は、たとえば `env SHINYTAURI_HEAVY_TESTS=1 mise run test:shinytest2` を実行します。

R のテストは `rhino/tests/testthat/` に置きます。`test-shinytest2*.R` を `test:shinytest2`、それ以外を `test:testthat` が実行します。重いケースには `testthat::skip_if_not(Sys.getenv("SHINYTAURI_HEAVY_TESTS") == "1")` を付けます。Rust では重いテストに `#[ignore]` を付けると、同じ環境変数で `test:tauri` が実行対象に加えます。
