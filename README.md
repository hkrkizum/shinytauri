# shinytauri
R shinyをtauriでwrapしWindows環境でスタンドアロンアプリとして配布できるようにする

## 開発時のチェック

初回はリポジトリのルートで `nix develop -c mise run setup` を実行します。
このクローンの pre-commit フックが有効になり、`git merge` は fast-forward できる場合もマージコミットを作成します。

開発シェルでは `mise run QA` で Nix・TOML・R のフォーマットと lint を確認できます。
フォーマットだけなら `mise run format`、lint だけなら `mise run lint` で、言語別のチェックを並列実行できます。
コミット時も同じタスクが作業ツリーをチェックし、問題があれば該当ファイルを報告してコミットを止めます。

`mise run QA-fix` は Nix・R の lint 違反のうち自動修正できるものを直し、Nix・TOML・R を整形します。
すべての修正が完了してから `QA` を実行し、違反が残っていれば失敗します。
