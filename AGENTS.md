# shinytauri

## 憲法（Constitution）

作業開始前に必ず参照し、以下の原則を遵守すること：

1. **テスト駆動開発（TDD）** — Red-Green-Refactorサイクルを厳守
2. **R言語ベストプラクティス** — roxygen2（英語記述）、devtoolsワークフロー
3. **SOLID原則** — 単一責任、拡張可能な設計
4. **YAGNI** — 必要な機能のみ実装
5. **DRY** — 重複排除（ただし数回の重複は許容）
6. **S7型安全性** — 全公開関数の引数・戻り値をS7クラスで型定義し、バリデーションを途切れなく伝播

## コミュニケーション

日本語で行うこと。

## 日本語のマニュアル

README と `docs/*.qmd` は常体で書く。対象読者は、既存の Shiny アプリを Windows 向けに配布する開発者である。執筆と推敲には [yomiyasu](.agents/skills/yomiyasu/SKILL.md) を使い、技術用語、コマンド、動作条件が実装と一致することを確かめる。表現の検査には `mise run docs:lint` を使う。

同梱したスキルは [nanaism/yomiyasu の c49d5f1](https://github.com/nanaism/yomiyasu/tree/c49d5f1bee91258e33e1febc3569c6c87be7669e/skills/yomiyasu) の `skills/yomiyasu/` に対応する。更新時は、同梱のライセンスを含めてディレクトリ全体を入れ替える。

## 開発ルール

- コード品質検証には `mise.toml` のQAタスクを一貫して使用すること
- roxygen2 ドキュメント（タイトル・説明・パラメータ等）はすべて英語で記述すること
- 関数のエクスポート前にドキュメントとテストが存在すること
- コミットは論理的な単位で、変更内容を明確にすること
