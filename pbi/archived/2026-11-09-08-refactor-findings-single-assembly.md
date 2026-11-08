# PBI C7: findings 単一組立化

## メタ
- 種別: refactor / 候補 C7
- 優先度: rank 8
- RICE: 1.92 (Reach 6 / Impact 2 / Confidence 80% / Effort 5)
- 先行: C8 の前提 (C7 -> C8)
- 見積: 5

## ユーザーストーリー
保守担当者として、findings の組立ロジックを `core/findings.rs` に一本化したい。なぜなら CLI 側の重複実装がドリフトの温床であり、単一の純粋関数に寄せればテストと再利用が容易になるから。

## 背景
CLI ローカルの重複実装 vs `core/findings.rs` 正典の対応表:

- stuck: `GAP_SECS` / `MIN_EDITS` + `detect_thrash`: `src/cli/mod.rs:1429-1433` vs `core/findings.rs:28-31,234`
- failures: `FrictionRow` + `split_friction`: `:1534,1544` vs `:175-214`
- inventory: `is_usage_measurable`: `:2139,2180` vs `:138`
- usage_by_time: `bucket_label`: `:2041` (正典へ寄せ)
- savings_suggestion: `:2289-2303` (正典へ寄せ)
- `views/prompts.rs:193-224` `skill_candidates`: FS 再読込 + `transcript::prompt_text_in` + クラスタ混在
- 正典の純粋組立 (`FindingsInput` 上の pure、試験 `:360-604`) を拡張点とする

方針:
- 純粋な組立は `core/findings.rs` (`FindingsInput` -> Findings) に寄せる
- views は fetch -> pure -> render にする (I/O と描画にロジックを持たせない)
- thrash 定数は `core/thrash.rs` へ hoist する
- プロンプト再読込は adapter/store 側へ移す

## BDD 受け入れシナリオ

```gherkin
Scenario: CLI と core が同じ findings を返す
  Given 同一の store と同一入力がある
  When CLI 経由と core の pure 関数で findings を組み立てる
  Then 両者の結果が一致する

Scenario: views に純粋ロジックが残らない
  Given リファクタ後の views/prompts と cli/mod がある
  When FS 再読込・bucket 分類・thrash 判定の定義を探す
  Then 定義は core/adapter/store 側にだけ存在する

Scenario: thrash 定数が一箇所から参照される
  Given core/thrash.rs に定数がある
  When CLI と core の thrash 判定を実行する
  Then 両者が同じ定数を参照する
```

## 受け入れ基準
- [ ] stuck / failures / inventory / usage_by_time / savings の組立が `core/findings.rs` の pure 経由に一本化される
- [ ] `src/cli/mod.rs` の重複定義 (1429-1433, 1534, 1544, 2041, 2139, 2180, 2289-2303) が削除または薄い委譲になる
- [ ] `views/prompts.rs:193-224` から FS 再読込が除去され adapter/store 経由になる
- [ ] thrash 定数が `core/thrash.rs` に集約され二重定義がない
- [ ] 既存試験 `core/findings.rs:360-604` が緑で parity 試験が追加される
- [ ] CLI 出力の差分がない (parity 確認)

## テスト戦略 (t-wada)
- 単体 (多数): `core/findings.rs` の pure 組立に対する synthetic fixture 試験 (stuck 境界、split_friction 分岐、measurable 判定、bucket 境界、savings 分岐)。既存 :360-604 を拡張
- Parity (中程度): 旧 CLI ロジック vs 新 pure の同一入力比較試験。移行期間のみ残し緑後に旧経路を削除
- E2E (最小): 代表的な store に対する CLI 表示のスナップショット比較 1 件
- Outside-In: parity 試験 (Red) -> pure へ寄せ -> views 委譲化 -> 緑 -> 重複削除

## 実装アプローチ
1. `core/thrash.rs` に定数を hoist し両側から参照させる
2. `FindingsInput` を拡張し不足の組立を pure 化する
3. CLI / views を fetch -> pure -> render の薄い層にする
4. プロンプト再読込を adapter/store に移動する
5. parity 試験で出力一致を確認し重複を削除する

## 技術的考慮事項
- 依存: C8 の前提。C8 着手前に完了させる
- 非機能: 出力互換を壊さない。性能劣化なし
- 禁止: views/cli への新規純粋ロジック追加

## 実装者向け注記
- 着手前に `grep -rn "detect_thrash\|split_friction\|is_usage_measurable\|bucket_label\|skill_candidates" src/` で重複の現状を確認する
- `docs/specs/` の該当 spec があれば同変更で更新する

## Definition of Done
- [ ] 全 BDD シナリオが自動試験として緑
- [ ] 重複定義がなく単一組立になっている
- [ ] pure-join-placement 則 (結合・分類・判定の pure ロジックは core に置き views/cli/adapter の描画・取得層に置かない) を DoD としレビューで確認した
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み

## 検証記録 (B4統合・2026-11-09)

- 実装: `stuck_episodes`/`friction_rows`/`inventory_rows` を `core/findings.rs` に純粋組立として追加、thrash定数を `core/thrash.rs` へhoist、view側はfetch→pure→render。`skill_candidates` はadapter `prompt_text_at` 経由に（FS直読排除）。
- 申送り事項: `skill_candidates` がpointer毎にtranscriptを1回読む（以前はfile毎1回）。batch reader化はadapter側の仕事として別PBI候補 — profilingで問題化したら再検討トリガー。
- 境界検証: fmt OK、clippy 0 errors、lib 644 passed、parity 7/7、prompts/pure-join/line-budget回帰 green。mod.rs 2905行でbudget内。
