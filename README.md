# shinytauri
R shinyをtauriでwrapしWindows環境でスタンドアロンアプリとして配布できるようにする

## 開発時のチェック

リポジトリのルートで `nix develop` を実行すると Nushell が起動します。mise のタスクも Nushell で実行されます。

初回はリポジトリのルートで `nix develop -c mise run setup` を実行します。
このクローンの pre-commit フックが有効になり、`git merge` は fast-forward できる場合もマージコミットを作成します。
`setup` の最後に Git の設定値とフックの実行権限を確認し、各項目の結果を表示します。開発シェルでは `mise run setup:check` でいつでも再確認できます。

開発シェルでは `mise run QA` で Nix・TOML・R のフォーマットと lint を確認できます。
フォーマットだけなら `mise run format`、lint だけなら `mise run lint` で、言語別のチェックを並列実行できます。
コミット時も同じタスクが作業ツリーをチェックし、問題があれば該当ファイルを報告してコミットを止めます。

`mise run QA-fix` は Nix・R の lint 違反のうち自動修正できるものを直し、Nix・TOML・R を整形します。
すべての修正が完了してから `QA` を実行し、違反が残っていれば失敗します。
