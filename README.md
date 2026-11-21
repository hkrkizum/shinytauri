# shinytauri

Shiny アプリと R を、オフラインで導入できる Windows 用インストーラーにまとめるプロジェクトです。標準のデモでは、2 個のサイコロを振った結果を集計し、操作できる表を含む Quarto ダッシュボードを HTML 1 ファイルに保存できます。別の Shiny アプリも配布設定 TOML で指定できます。

## Getting Started — Windows で配布版を使う

必要なものは、Microsoft Edge WebView2 Runtime 導入済みの Windows 10/11 x64 PC と、配布担当者が用意した `shinytauri_<バージョン>_x64-setup.exe` です。デモには R・実行用パッケージ・Quarto が同梱されています。インストールからレポートの保存までオフラインで行えます。

1. インストーラーを PC にコピーして実行し、スタートメニューから **shinytauri** を開きます。
2. 「サイコロ集計」で試行回数を選び、「サイコロを振る」を押します。集計中も「1 を足す」でカウンターを操作できます。
3. 集計が完了したら「レポート」を開き、「レポートを作成」を押します。作成が完了すると表示される「ダウンロード」をクリックし、`dice-dashboard.html` を保存します。
4. 保存した HTML をブラウザーで開きます。「合計」や「理論割合 (%)」で分布表を絞り込めます。別の PC で閲覧するときは HTML だけをコピーしてください。

レポートには「レポートを作成」を押した時点のカウンター値と、直近に計算した出目別件数が入ります。作成中の入力変更も次の集計も、保存される値に影響しません。アプリ内のページを移動しても、同じセッションの集計結果とカウンターは保持されます。

## 開発・ビルドに必要なツール

| 用途 | 前提と環境 |
| --- | --- |
| Linux で開発・テスト | x86_64 Linux、Git、flakes と `nix-command` が有効な Nix。デスクトップ版の起動にはグラフィカルセッションを使います。 |
| Linux で Windows 版をビルド | 同じ Git と Nix。専用シェル `.#windows` が Rust の Windows ターゲット、MinGW-w64、NSIS、rig、Nushell、mise を提供します。 |
| Windows でネイティブビルド | Windows x64、Git、Visual Studio Build Tools（「C++ によるデスクトップ開発」と Windows SDK）、rustup（x64 MSVC）、rig 0.11.1、Nushell、mise、Tauri CLI。下記のビルド手順を参照してください。 |

ソースからの初回セットアップとインストーラーのビルドにはインターネット接続が必要です。Linux の開発シェルには Rust、Tauri CLI、WebKitGTK、rig、Quarto、Chromium、mise、Nushell が含まれます。rig がプロジェクトのロックに合わせて R 4.6.2 と R パッケージを復元します。

### ソースからデモを起動する

x86_64 Linux で次を実行します。`setup` はこのクローンの Git フックを有効にし、デモの R 環境を復元します。

```sh
git clone https://github.com/hkrkizum/shinytauri.git
cd shinytauri
nix develop -c mise trust
nix develop -c mise run setup
nix develop -c mise run run:tauri
```

`run:tauri` は `cargo tauri dev` を実行し、デモを Tauri のウィンドウに表示します。ブラウザーで開発する場合は、リポジトリのルートで `nix develop` に入り、`cd src-shiny`、`rig run dev.R` の順に実行して `http://127.0.0.1:3838` を開いてください。いずれも開発時は同じポートを使うため、切り替えるときは起動中の方を終了してください。

## 技術スタック

| 役割 | 技術と使い方 |
| --- | --- |
| デスクトップ・配布 | Rust / Tauri 2 が同梱 R を起動し、Windows の WebView2 にローカルの Shiny を表示します。インストーラーは NSIS 形式です。 |
| デモの画面とデータ | Shiny / Rhino と box でモジュールを構成し、bslib と shiny.router で画面とナビゲーションを提供します。S7 が集計データの型を、DT が絞り込み可能な表を担当します。 |
| バックグラウンド処理 | 各セッションの `shiny::ExtendedTask` が計算とレンダリングを mirai に渡します。アプリ起動時に 4 つのローカルデーモンを立ち上げ、終了時に停止します。[mirai の Shiny 統合ガイド](https://mirai.r-lib.org/articles/shiny.html) |
| レポート | Quarto と knitr が集計時点の値と DT 表を埋め込んだダッシュボードを生成します。R の後処理でスタイルシート・入れ子の CSS `@import`・スクリプトの `data:` URL を復号し、HTML 内の `<style>` と `<script>` に展開します。 |
| 環境と検証 | Nix flake と rig のロックでツールと R 依存を管理します。mise / Nushell から testthat、shinytest2（Chromium）、cargo-nextest、配布構成テストを実行します。 |

デモでは計算中の操作を試せるよう、サイコロ集計に 2 秒の待ち時間を設けています。「レポート」の表で合計「2」を選ぶと、理論割合 2.8% の欄に `1 / 2` と表示されます。これは「2」と「12」のうち、現在表示している行が 1 件という意味です。保存した HTML でも同じ操作ができます。画像とフォントは HTML 内に埋め込まれます。インライン CSS や JavaScript を制限するプレビューサービスでは、ファイルをローカルブラウザーで開いてください。

## ディレクトリ構成

| パス | 内容 |
| --- | --- |
| `src-shiny/` | 既定のデモ。`app.R` が入口、`app/main.R` がページとモジュールを接続します。`tests/testthat/` に単体・ブラウザーテストがあります。 |
| `src-shiny/app/` | `view/` に画面と共有の表、`logic/` に集計・レポート生成、`types.R` に型検査、`report/dashboard.qmd` にレポートのレイアウトがあります。 |
| `src-tauri/`、`ui/` | Rust のデスクトップホスト、Tauri 設定・アイコン・テストと、Shiny の起動を待つための HTML です。 |
| `bundles/`、`examples/minimal/` | デモの配布設定 `rhino.toml`、最小アプリ用の `minimal/bundle.toml` とロック、素の Shiny アプリの例です。 |
| `scripts/` | Windows ビルド、同梱 R の起動、Git フック設定の確認、配布構成のテストを置きます。 |
| `.githooks/`、`mise.toml` | コミット・マージ時のフックと、セットアップ・QA・ビルドのタスク定義です。 |
| `flake.nix`、`flake.lock`、`nix/`、`rust-toolchain.toml` | 開発用・クロスビルド用のシェル、rig の Nix 定義、Rust のバージョンを管理します。 |
| `rproj.toml`、`rproj.lock` | ルートはビルド機用、`src-shiny/` はデモ用、`bundles/minimal/` は最小アプリ用の R 依存定義とロックです。 |

デモの R 依存を追加するときは `src-shiny/` で `rig proj add パッケージ名` を実行し、更新されたマニフェストとロックを一緒にコミットします。マニフェストを手で編集した場合は `rig proj lock` でロックを更新します。[rig のプロジェクトガイド](https://rig.r-lib.org/proj-guide.html)

ロックファイルには R 4.6.2 の x86_64 Linux 用と x64 Windows 用の依存関係を記録しています。`mise run setup:r` は `src-shiny/` で `rig proj sync --locked` を実行します。rig はロックされた R がユーザー領域に未導入ならインストールし、パッケージを `src-shiny/.rvenv/lib/` に復元します。ロックファイルの更新後も同じタスクを使えます。

## Windows x64 用オフラインインストーラー

### ビルド

ネットに接続できるビルド機で、リポジトリのルートから実行します。x86_64 Linux では Windows 用の Nix シェルを使えます。標準デモと最小アプリは、それぞれ次のコマンドでビルドします。

```sh
nix develop .#windows -c mise trust
nix develop .#windows -c mise run build:windows
nix develop .#windows -c mise run build:windows bundles/minimal/bundle.toml
```

Windows x64 でネイティブビルドする場合は、上記の前提ツールを用意し、PowerShell で Tauri CLI を導入してビルドします。

```powershell
cargo install tauri-cli --version 2.12.6 --locked
mise trust
mise run build:windows
```

成果物は `src-tauri/target/<target>/release/bundle/nsis/` に出力されます。`<target>` は Linux ビルドで `x86_64-pc-windows-gnu`、Windows ビルドで `x86_64-pc-windows-msvc` です。

### 配布するアプリの指定

配布設定 TOML を引数に渡します。設定はアプリの外に置けます。`bundles/minimal/bundle.toml` の例では、アプリのソースは `examples/minimal/app.R` と `www/` だけです。

```toml
app         = "../../examples/minimal"
files       = ["app.R", "www"]
lock        = "rproj.lock"
productName = "shinytauri-minimal"
identifier  = "io.github.hkrkizum.shinytauri.minimal"
```

- `app` と `lock` は配布設定ファイルからの相対パスです。`app` は `shiny::runApp(".")` で起動できるディレクトリを指します。`files` にはアプリを基準に、実行に使うファイルやディレクトリを列挙します。ディレクトリの中身は階層を保ってコピーされるため、`ui.R`・`server.R` 形式や独自の `data/` も同じ方法で指定できます。
- `lock` は配布先で使う R とパッケージの rig ロックです。既存のアプリ用ロックを指定するか、最小例のように配布設定の隣に `rproj.toml` を置きます。そのプロジェクトの `[dependencies]` に `shiny` とアプリが実行時に使うパッケージを宣言し、そのディレクトリで `rig proj lock` を実行します。ロックには対応する R バージョンの Windows x64 ターゲットと `main` の Windows バイナリが必要です。
- `quarto = true` で Quarto CLI を同梱します。既定値は `false` です。別製品として配布する場合は固有の `productName` と `identifier` を指定します。既定値はリポジトリの Tauri 設定に従います。

ビルド機用の R とビルド用パッケージ（`RcppTOML`、`jsonlite`、`filelock`）は、リポジトリ直下の `rproj.toml` と `rproj.lock` で管理します。ビルドタスクはルートで `rig proj sync --locked` を実行し、`rig run --rscript` でビルドスクリプトを起動します。

### 同梱内容と起動の仕組み

[shinyelectron の bundled 方式](https://github.com/coatless-rpkg/shinyelectron)を参考に、ポータブル R とアプリ専用ライブラリを Tauri のリソースとして配置します。ビルドタスクは配布設定で指定したロックの Windows x64 ターゲットを読み、対応する [portable-r-windows](https://github.com/portable-r/portable-r-windows/releases) の ZIP の SHA-256 を照合します。続いて `main` グループの Windows バイナリをロックされた URL から取得し、パッケージ名とバージョンを照合します。アプリは `offline/app/` に置き、Tauri の共通ランチャーがそこを作業ディレクトリとして起動します。配布する R を更新するときはロックの更新と合わせて `scripts/build-windows.R` の公式配布物の SHA-256 も更新してください。

Quarto を使用する場合は Quarto CLI 1.11.9 の公式 Windows ZIP を取得し、固定した SHA-256 を照合して `offline/quarto/` に展開します。配布版の Tauri は同梱 R を直接起動し、同梱 Quarto のパスを `SHINYTAURI_QUARTO` でアプリに渡します。開発時は `nix develop` が用意する `quarto` を使用します。[Quarto CLI の公式リリース](https://github.com/quarto-dev/quarto-cli/releases/tag/v1.11.9)

Shiny がループバックの空きポートで待ち受けてから、その URL をウィンドウに表示します。デスクトップアプリを閉じると Windows のジョブオブジェクトが R と mirai の子プロセスを終了します。起動に失敗した場合はウィンドウタイトルに表示されるログを確認してください。

### 配布前のオフライン確認

1. WebView2 Runtime 導入済みで、R と Quarto が入っていない Windows x64 のテスト環境を用意します。
2. インストーラーをコピーしてネットワークを切断し、インストールから初回起動まで実行します。
3. 「1 を足す」でカウントが変わることと、「サイコロを振る」で計算結果が表示されることを確認します。
4. 「レポート」ページで「レポートを作成」を押し、「サイコロ集計」に戻ってカウンターを変更します。「レポート」に戻り、「ダウンロード」で HTML を保存し、ブラウザーで開いて作成時点のカウンター値・試行回数と照合します。
5. ウィンドウを閉じ、同梱 R のプロセスが終了することと、オフラインのまま再起動できることを確認します。

## 開発時のチェック

リポジトリのルートで `nix develop` を実行すると Nushell が起動します。mise のタスクも Nushell で実行されます。

### Git フック

`mise run setup` はクローンごとに `core.hooksPath=.githooks` と `merge.ff=false` を設定します。これにより、通常の `git merge` は fast-forward が可能な場合もマージコミットを作成し、テスト結果を待ちます。`mise run setup:check` で設定値と両フックの実行権限を確認できます。

| Git 操作 | コミット作成前に実行する検証 |
| --- | --- |
| `main` への直接の `git commit`（`--amend` を含む） | `pre-commit` が QA の実行前に拒否します。変更は `dev` などの作業ブランチでコミットしてから `main` にマージします。 |
| 作業ブランチでの通常の `git commit` | `pre-commit` が `mise run QA` を実行します。 |
| `git merge` | `pre-merge-commit` が `mise run QA-full` を実行します。フォーマット、lint、重いケースを含む全テストが成功するとマージコミットを作成します。 |
| 競合解消後や `git merge --no-commit` 後の `git commit` | `pre-commit` が進行中のマージを検出し、同じ `QA-full` を実行します。 |

フックは Nix 開発シェルを自動で起動します。検証で止まった場合は出力されたエラーを解消し、`git status` でマージが進行中なら `git commit` で検証を再実行できます。

開発シェルでは `mise run QA` で Nix・TOML・R・Nushell のフォーマットと lint を確認できます。
フォーマットだけなら `mise run format`、lint だけなら `mise run lint` で、言語別のチェックを並列実行できます。

`mise run QA-fix` は Nix・R・Nushell の lint 違反のうち自動修正できるものを直し、Nix・TOML・R・Nushell を整形します。
すべての修正が完了してから `QA` を実行し、違反が残っていれば失敗します。

### テスト

開発シェルにはブラウザーテスト用の Chromium が含まれます。`mise run setup:r` で testthat と shinytest2 を含む開発用パッケージを復元してください。

| コマンド | 実行内容 |
| --- | --- |
| `mise run test:bundle` | Rhino と最小 Shiny アプリの配布ファイルおよび実行時依存を検証 |
| `mise run test` | Rhino の単体テスト、shinytest2 のブラウザーテスト、Tauri の nextest、配布構成テストを並列実行 |
| `mise run test-full` | 同じテストで重いケースも実行 |
| `mise run QA-ci` | `QA` と `test` |
| `mise run QA-full` | `QA` と `test-full` |

通常のブラウザーテストは、カウンターの操作と、レポート作成中の入力変更・ダウンロード内容を確認します。重いテストでは10万回のサイコロ集計を起動し、実行中の応答と完了後の結果を確認します。`test-full` は `SHINYTAURI_HEAVY_TESTS=1` を設定して `test` を呼び出します。個別に試す場合は、たとえば `env SHINYTAURI_HEAVY_TESTS=1 mise run test:shinytest2` を実行します。

R のテストは `src-shiny/tests/testthat/` に置きます。`test-shinytest2*.R` を `test:shinytest2`、それ以外を `test:testthat` が実行します。重いケースには `testthat::skip_if_not(Sys.getenv("SHINYTAURI_HEAVY_TESTS") == "1")` を付けます。Rust では重いテストに `#[ignore]` を付けると、同じ環境変数で `test:tauri` が実行対象に加えます。
