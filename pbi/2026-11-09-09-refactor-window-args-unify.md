# PBI C5: WindowArgs 単一化リファクタ

- 種別: refactor
- 優先度: rank 9
- RICE: 1.33 (Reach 5 / Impact 1 / Confidence 80% / Effort 3)
- Estimate: 3
- Status: candidate

## 背景

`src/cli/window.rs` で期間指定引数が二重管理されている。

- `src/cli/window.rs:29-60`: `PeriodArgs` / `PairArgs` がフィールド同一 (`since`, `until`, `last`) でドキュメントのみ相違
- `src/cli/window.rs:164-286`: `open` / `open_scoped` / `open_pair` / ... に分割
- `src/cli/window.rs:293-300` `parse_period` vs `src/cli/window.rs:308-315` `parse_pair`: パース重複
- `src/cli/window.rs:575-665`: 11-arm の destructure が肥大化
- `src/core/period.rs:40-104` vs `src/core/period.rs:133-`: previous 期間の再導出が重複

機能追加のたびに両系統へ同等修正が必要で、齟齬リスクが高い。

## 提案

- 単一 `WindowArgs { since, until, last }` に統一
- `mode: Single | Pair` で単体/比較を区別
- 単一 `parse_window` に集約
- `resolve_pair = resolve + derive_previous` として残し、再導出を core に一本化

## 受け入れ条件

1. `PeriodArgs` / `PairArgs` が `WindowArgs` に統一され、重複構造体が削除されること
2. `parse_period` / `parse_pair` が単一 `parse_window` に集約されること
3. `resolve_pair` が `resolve + derive_previous` の合成として定義されること
4. 11-arm destructure 箇所が簡素化され、arm 数が削減されること
5. `--help` 出力がバイト同一であること (help capture で固定)
6. 既存の期間系テストが全てグリーンのまま通ること

## BDD シナリオ

```gherkin
Scenario: 単一期間指定が従来通り解決される
  Given 単一モードで --since/--until/--last のいずれかを指定する
  When parse_window + resolve を実行する
  Then 従来の parse_period + resolve と同一の期間が得られる

Scenario: 比較期間指定が従来通り解決される
  Given Pair モードで期間を指定する
  When parse_window + resolve_pair を実行する
  Then 従来の parse_pair + 個別導出と同一の (current, previous) が得られる
```

## テスト戦略 (t-wada)

- Red: `parse_window` が `parse_period` / `parse_pair` と同一結果を返すパリティテスト (単体/Pair 各1以上)
- Green: `WindowArgs` + `mode` + `parse_window` + `resolve_pair` 合成で最小実装
- Refactor: 旧構造体・旧パース・11-arm destructure を削除/簡素化し、help capture で出力同一を確認
- 既存期間系テストを回帰スイートとして維持

## DoD

- [ ] BDD 2 シナリオ以上が自動テストで担保される
- [ ] help capture テストがグリーン (出力バイト同一)
- [ ] `cargo test` / `cargo clippy` がグリーン
- [ ] 重複構造体・重複パースが残存しない

## 検証記録 (B4統合・2026-11-09)

- 実装: `WindowArgs`＋`WindowMode`＋単一 `parse_window`、resolve_pairはresolve＋derive_previous合成。PeriodArgs/PairArgsはhelp文言保持の薄シムとして残置（mod.rs側Command宣言のため）。
- ついでの統合修正: `views/trend.rs:664` のdocが削除済み `parse_pair` に言及していたため `parse_window` に1語修正。
- 境界検証: fmt OK、clippy 0 errors、help golden 5/5（バイト同一）、period/window語彙テスト green、lib 644 passed。
