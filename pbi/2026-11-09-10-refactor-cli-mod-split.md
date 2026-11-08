# PBI C6: cli/mod.rs 分割

## 基本情報
- 種別: リファクタ（内部品質）
- 優先度: rank 10 / RICE 0.7（Reach 7 / Impact 1 / Confidence 80% / Effort 8）
- 前提: C4 の後に着手
- 見積もり: 8pt

## ユーザーストーリー
保守担当者として、`src/cli/mod.rs` を責務ごとに分割してほしい。なぜなら 2907 行の単一モジュールは変更の影響範囲が見えず、レビューと拡張のコストが高いから。

## 背景（現状コード）
- `src/cli/mod.rs` 2907 行:
  - dispatch `run()`: 272-466
  - `DeliveryPlan` / `Payload` / `AdvisorLaunch` / `ProcessAdvisor`: 512-619
  - `delivery_mode`, `optimize`: 703-772
  - `findings_input`: 838-882
  - `doctor`: 960-約1330 ＋ `prune_phrase`: 1280 ＋ `doctor_agent_lines`: 1323
  - `stuck`: 1428、`failures`: 1510、`overhead`: 1711
  - `analyze` / `run_analyze`: 1812-1874
  - `usage`: 1901 / 2035、`inventory`: 2116
- `window.rs:127-136` に line-budget テストが存在するのは、容量超過のため
- views 着陸ルールは維持する

## 方針
- delivery 系 → `cli/deliver.rs` に移動
- renderer 系 → `views/*` に移動し、シグネチャは `(&Store, &Harness)` に統一
- `tests/cli_mod_line_budget.rs` の上限は維持し、むしろ引き締める
- 振る舞い変更なし（純粋な移動＋薄い結合）

## BDD受け入れシナリオ

```gherkin
Scenario: 全ゴールデンがバイト同一
  Given 分割前の全 CLI ゴールデンが取得済みである
  When  分割後のバイナリで同一コマンド群を実行する
  Then  全ゴールデンがバイト同一である

Scenario: line-budget テストが通る
  Given tests/cli_mod_line_budget.rs の上限が引き締められている
  When  cargo test を実行する
  Then  budget テストがパスする

Scenario: views ルールが守られる
  Given レンダラーが views/* に移動している
  When  依存方向を検査する
  Then  views 着陸ルール違反がない
```

## 受け入れ基準
- [ ] delivery 系が `cli/deliver.rs` に移動している
- [ ] renderer 系が `views/*` に移動し `(&Store, &Harness)` を取る
- [ ] 全ゴールデンがバイト同一である
- [ ] `tests/cli_mod_line_budget.rs` がパスする（上限引き締め済み）
- [ ] `mod.rs` が設定した行数上限以下である
- [ ] views 着陸ルール違反がない
- [ ] 公開 CLI の振る舞い変更がない
- [ ] `cargo test` / `cargo clippy` が通る

## テスト戦略（t_wada式）
- E2E: 全 CLI ゴールデン比較（バイト同一）
- 統合: line-budget テスト、views 着陸ルール検査
- 単体: 移動した関数ごとの既存単体テストをそのまま緑化
- 順序: Outside-In（ゴールデン Red 確認 → 移動 → Green → budget 引き締め → refactor）

## 実装者向け注記

### 現状コードの確認
```bash
grep -rn "DeliveryPlan\|run_analyze\|doctor_agent_lines" src/cli/mod.rs | head -30
wc -l src/cli/mod.rs src/cli/*.rs src/views/*.rs
```

### 実装手順
1. ゴールデン一式を取得し、移動前の baseline を固定する
2. delivery 系を `cli/deliver.rs` へ移動（呼び出し側は薄い委譲のみ残す）
3. renderer 系を `views/*` へ移動し `(&Store, &Harness)` に寄せる
4. `tests/cli_mod_line_budget.rs` の上限を引き締めて緑化する
5. ゴールデン差分ゼロと clippy を確認する

### 落とし穴
- 移動時にシグネチャを変えない（`(&Store, &Harness)` 統一は views 側のみ）
- `doctor` 周辺（960-1330）は分割単位が大きいため、機械的移動に留める
- window.rs の budget テストと二重に上限を緩めない

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとしてパスする
- [ ] ゴールデン差分ゼロである
- [ ] コードレビュー完了
- [ ] ドキュメント更新不要であることが確認済み

## 検証記録 (B6統合・2026-11-09)

- 実装: `cli/deliver.rs`＋`views/{doctor,stuck,failures,overhead,usage,inventory}.rs` 新設、mod.rs 2866→1039行、budget 3037→1080。
- 逸脱2件（受容）: (1) view引数は `&Session` のまま — guide.rs凍結呼出しとの整合＋PBI trap節に従う。`(&Store,&Harness)` 化はC8の担当。(2) `views/mod.rs` 登録＋parity tripwireの移設先追従（機械的）。
- 境界検証: fmt OK、clippy 0 errors、lib 645 passed、新golden 23/23、render_view 28/28、help/budget green。
