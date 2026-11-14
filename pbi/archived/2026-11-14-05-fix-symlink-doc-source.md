# PBI: symlink 非追従 doc の正典化

## ユーザーストーリー
将来の保守者として、「symlink は解決しない」という方針の正典を
一箇所で読みたい。なぜなら現在4箇所にほぼ同文があり、将来の文言修正
(dangling link の扱い追記等) が4箇所同時編集になり、spec-sync 運用下で
正典が曖昧だからだ。

## ビジネス価値
- 文言変更の単一箇所化 (将来の編集漏れ防止)。振る舞い変更なし
- 測定方法: `cargo doc` 上で方針の主説明が一箇所にあり、他が参照になっていること

## 現状確認 (Phase 0 済み)
- 4箇所の重複を確認済み (`src/cli/cleanup.rs` の `Entry.bytes` 付近、
  `sized_path`、`collect_entries`、`dir_size`)
- 帰属自体は正しい (前回レビューで修正済み)。今回は参照化のみで振る舞いに触れない
- spec-sync の観点では仕様文言の正典明確化に相当する

## BDD受け入れシナリオ

```gherkin
Scenario: 正典が一箇所にある
  Given 保守者が symlink 方針の説明を探している
  When  sized_path の doc を読む
  Then  方針の全文がそこにある

Scenario: 他は参照になっている
  Given 保守者が Entry.bytes の doc を読んでいる
  When  symlink の扱いの詳細を知りたくなる
  Then  sized_path への参照があり、全文の重複がない
```

## 受け入れ基準
- [x] 方針全文の正典が `sized_path` に一箇所ある
- [x] 他3箇所が参照に留まり、全文の重複がない
- [x] `cargo doc` の警告が出ない (壊れたリンクなし)
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 不要 (doc コメントのみの変更。既存テストが回帰網になる)

### 統合テスト
- 不要

### 単体テスト
- 不要。ただし `cargo doc --no-deps` の警告ゼロを完了条件にする

## 実装アプローチ
- **Outside-In**: 対象外 (振る舞い変更なしのため、既存 green を起点にする)
- **Red-Green-Refactor**: 1件ずつ参照化し、そのつど `cargo doc` 警告ゼロを確認する
- **リファクタリング**: 該当なし

## 見積もり
1 (要チームでの見積もり。doc 4箇所のみ)

## 技術的考慮事項
- 依存関係: なし (コメントのみ)
- テスタビリティ: `cargo doc` の警告を検出器にする
- 非機能要件: rustdoc の intra-doc link (`[`sized_path`]`) が解決できる形にすること

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること）
```bash
# 重複4箇所の再確認
grep -n "never resolve\|as a link instead" src/cli/cleanup.rs
```

### 実装手順
1. `sized_path` の doc を正典として確定する (現文で十分か読む)
2. 他3箇所を参照に短縮する (1箇所ずつ)
3. `cargo doc --no-deps` の警告ゼロを確認する
4. 全テストを Green のまま保つ

### 落とし穴
- intra-doc link の記法ミスで警告が出る。`cargo doc` で毎回確認すること
- ついでに振る舞いを変えない (文言層に留めること)

## Definition of Done
- [x] 全BDDシナリオが確認されパスする (`cargo doc` 警告ゼロを含む)
- [x] コードレビュー完了
- [x] ドキュメント更新済み
