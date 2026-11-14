# PBI: `--list` の未読エントリ件数の可視化

## ユーザーストーリー
掃除対象を確認する運用者として、`--list` の合計サイズが実態より小さく
見える事態を避けたい。なぜなら権限エラー等で読めなかったエントリが
無警告で黙殺されると、「空きが足りないのに小さいと出る」という混乱になるからだ。

## ビジネス価値
- 黙殺の可視化 (読めなかった件数の把握)。通常表示の合計は現状維持し、
  詳細は `--verbose` 時や `doctor` 出力にだけ出す
- 測定方法: 読めないエントリがある状態で `--list` しても失敗せず、
  `--verbose` 時にスキップ件数が確認できること

## 現状確認 (Phase 0 済み)
- `src/cli/cleanup.rs` の `collect_entries` / `dir_size` 内で
  `let Ok(meta) = ... else { continue; }` により無警告スキップされることを確認済み
- sweep 側の `skipped` (victim 集合) とは別物。今回触るのは一覧側のみ
- 通常出力の変更はしない方針 (合計の過小表示自体は既存挙動として温存)

## BDD受け入れシナリオ

```gherkin
Scenario: 読めないエントリがあっても一覧は失敗しない
  Given 権限等で読めないエントリを含むエリアがある
  When  --list を実行する
  Then  一覧は成功し、読めた分が表示される

Scenario: verbose でスキップ件数が分かる
  Given 権限等で読めないエントリを含むエリアがある
  When  --verbose 付きで --list を実行する
  Then  スキップ件数が確認できる

Scenario: 通常表示は変わらない
  Given 権限等で読めないエントリを含むエリアがある
  When  --list を実行する
  Then  通常出力は従来とバイト同一である
```

## 受け入れ基準
- [x] 読めないエントリがあっても `--list` がエラーにならない (現状維持)
- [x] スキップ件数が stderr 警告として出る（正常時は無出力。`--verbose` ゲートは
  `mod.rs` 変更と tripwire 引き上げを要するため見送り、既存の stderr 診断線
  （freshness ヘッダと同 channel）に寄せた）。通常・JSON 出力はバイト同一
- [x] 通常出力・JSON 出力はバイト同一 (機械契約に触れない)
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 読めないエントリを含む合成エリアでの `--list` 成功テスト (chmod 000 等)

### 統合テスト
- スキップ件数の表テスト (0 件 / 1 件 / 複数件)

### 単体テスト
- 件数集計ロジックの境界値テスト (全件読めない場合を含む)

## 実装アプローチ
- **Outside-In**: E2Eテストから開始し、失敗を確認してから実装
- **Red-Green-Refactor**: TDDサイクルを各レイヤーで適用
- **リファクタリング**: グリーンになるたびに品質改善

## 見積もり
2 (要チームでの見積もり。内訳目安: 件数伝搬 1、表示+テスト 1)

## 技術的考慮事項
- 依存関係: `collect_entries` の戻り値の形に影響する。呼び出し元
  (`list_areas`、`sweep` 系は触らない) を確認してから形を決める
- テスタビリティ: chmod 000 は root 実行だと効かない。テストは
  `cfg(unix)` + 非 root 前提の既存パターンに従い、CI 条件を合わせる
- 非機能要件: 通常・JSON 出力不変。件数表示の追加先は交渉可能
  (`--verbose` か `doctor` かは実装時に決める)

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること）
```bash
# 黙殺箇所の再確認
grep -n "let Ok(meta)" src/cli/cleanup.rs
# collect_entries の呼び出し元
grep -rn "collect_entries" src/ tests/ | head -n 20
# 既存の権限系テストのパターン
grep -rn "chmod\|PermissionsExt\|from_mode" src/cli/cleanup.rs | head
```

### 実装手順
1. E2Eテストを Red で書く (読めないエントリ混じりの `--list` 成功 + 件数表示)
2. 件数集計の統合テストを Red で書く (0/1/複数)
3. 件数を運ぶ形にして Green にする (戻り値拡張または並行返却)
4. `--verbose` (または `doctor`) にだけ表示を足す (通常・JSON 不変を確認)

### 落とし穴
- root 実行では chmod 000 が効かずテストが偽陽性になる。既存の unix ゲートに合わせること
- sweep 側の `skipped` と混同しない (別概念・別経路であること)
- JSON 出力に紛れ込ませない (機械契約の golden が壊れる)
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] テストカバレッジが基準を満たす (E2E/統合/単体すべて)
- [x] コードレビュー完了
- [x] リファクタリング完了 (グリーン後)
- [x] ドキュメント更新済み
