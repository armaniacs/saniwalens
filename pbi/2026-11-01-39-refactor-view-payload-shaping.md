# PBI: view に 1 つの typed payload を持たせる（trend から、golden JSON contract 同梱）― payload-shaping slice

## ユーザーストーリー
実装者として、view の出力が 1 つの typed payload から導出されていてほしい、
なぜなら table cells と JSON が別々に手作りされていて、精度規則の不統一が
実 drift 例として観測されており、golden JSON contract テストが 0 件だからだ。

## 優先度
- 順位: 39 / 全候補数 7
- RICEスコア: 6.0（Reach=3 / Impact=3 / Confidence=2 / Effort=3日）
- 根拠: 6.0 tier で最優先 ― 実 drift 例（table と JSON が食い違いうる）が
  観測済みで、リスク軽減効果が最も大きい。tie-break: リスク軽減 →
  時間的緊急性

## ビジネス価値
- JSON machine contract（cli.md:141-163）の形が view のソースにしか存在しない
  状態が消え、最初の golden JSON contract テスト（view-landing rule の発火時
  義務）が先に用意される
- 測定方法: trend の table と JSON が 1 つの typed payload から導出され、
  精度規則が 1 箇所に寄ること。golden テストが JSON 契約を pin すること

## 背景（なぜなぜ分析 2026-11-01 round3、3 立場 x 30 段階、file:line 実証）
- 配送 seam が render()/emit_json で止まり（report.rs:43-54、render は
  Format::Json を unreachable!() :54）、payload の形が 12 view に shallow に
  残置 → 「同じデータの 2 表現」（Vec<Vec<String>> セル vs ad-hoc
  serde_json::Value）が型で表現されず、両者の対応を主張する pin が発行できない
- **新規発見（実 drift 例）**: trend.rs:84-96 metric_json_float は生 f64 を
  比較、trend.rs:221-232 delta_cells_float_opt は表示精度（0.1）で判定 →
  3.3s vs 3.3000000000000003s（ms 派生 duration の加算誤差で現実に起こりうる）
  で table は "+0.0|same"、JSON は "4.4e-16|up" と食い違う。table 側のみ
  pin 済み（trend.rs:697-721）、JSON 側 drift は pin なし
- golden JSON 契約テスト = 0 件、E2E --format json テスト = 0 件（grep 実測）
- precedents は既に存在するのに標準化されていない: trend_json（返り値を返す
  payload 関数、trend.rs:123-181 + pin :724-809）と Findings（Serialize
  derive、core/optimize.rs:18-19）
- 根本原因: 配送 seam が render() で止まり、payload の形が 12 view に shallow
  に残置。契約知識が 5 箇所（形/規約/受入基準/pin 方法/line budget）に散在

## BDD受け入れシナリオ

```gherkin
Scenario: table と JSON が 1 つの payload から導出される
  Given trend の typed payload がある
  When table rendering と JSON rendering を作る
  Then 両者は同じ payload から導出され、精度規則は 1 箇所に寄る

Scenario: 精度規則の drift が pin される
  Given table と JSON の判定が 1 箇所に寄った
  When 3.3s vs 3.3000000000000003s のような加算誤差の入力を与える
  Then table と JSON は同じ判定（same/up）を出す（golden が pin）

Scenario: 出力はバイト同一である
  Given capture で検証済みの trend 出力がある
  When 本 PBI を着地させる
  Then 3 format の出力はバイト同一で、エラーにはならない
```

## 受け入れ基準
- [ ] trend の payload を typed serializable にする
  （table と JSON がそこから派生。trend_json precedent（:123-181 + pin
  :724-809）を標準形として使う）
- [ ] metric 比較の精度規則を 1 箇所に寄せる
  （metric_json_float の生 f64 比較 :84-96 と delta_cells_float_opt の表示
  精度判定 :221-232 の不統一を消す）
- [ ] 最初の golden JSON contract テストを同梱する
  （view-landing rule の発火時義務を先に用意）
- [ ] 出力 byte-identity: trend x 3 format の出力不変
  （capture pin 保護 trend.rs:697-721/:724-809、trend_markdown.rs 4 goldens、
  period_errors.rs）
- [ ] with_window 添付の退化禁止（trend 手動 window :167-171）
  sql/analyze 非添付（:1166, :2128）は本 PBI のスコープ外
- [ ] spec 同一変更: cli.md:141-163 machine contract 節 +
  architecture.md:101 report 行 + cli-view-addition.md:36-40 golden "None
  exist yet" の解消
- [ ] delivery-return（render が返り値を返す）は本 PBI のスコープ外
  （後続、ledger 記録）; doctor text pin 解禁も後続
- [ ] tripwire に payload 型は view ファイル / 新 module へ（mod.rs に足さない）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- trend x 3 format の出力不変（capture pin）

### 統合テスト
- payload → table/JSON 導出の一致（精度規則の drift pin 込み）

### 単体テスト
- golden JSON contract テスト（最初の 1 本）
- 精度規則の境界（3.3s vs 3.3000000000000003s で table/JSON が同じ判定）

## 実装アプローチ
- **Outside-In**: 精度規則 drift の Red から、payload 型化 → golden pin →
  spec 更新の順
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=3日。RICE 6.0。縦スライス: trend から payload-shaping 先、
delivery-return は後続）

## 技術的考慮事項
- 依存関係: PBI 32（trend.rs 分割、完了済み）が前提。delivery-return（後続）が
  依存。PBI 36-38/40-42 と独立
- テスタビリティ: payload は純粋構造、golden で pin
- 非機能要件: プライバシー（fixture は合成）、出力 byte-identity

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- src/cli/trend.rs: metric_json_float :84-96（生 f64 比較）、
  delta_cells_float_opt :221-232（表示精度 0.1 判定）、trend_json :123-181
  （precedent: 返り値を返す payload 関数）、pin :724-809、table pin :697-721、
  JSON 分岐 :405、手動 window :167-171
- src/cli/report.rs: render :43-54（unreachable!()）、emit_json
- src/core/optimize.rs:18-19: Findings（Serialize derive precedent）
- docs/specs/cli.md:141-163 machine contract 節、architecture.md:101 report 行、
  .claude/rules/cli-view-addition.md:36-40 golden "None exist yet"

### 実装手順
1. Red: 精度規則 drift の pin（table/JSON が同じ判定を出す期待）を書く
2. trend の payload を typed serializable に（trend_json precedent を標準形に）
3. 精度規則を 1 箇所に寄せる
4. golden JSON contract テストを同梱する
5. spec 同一変更（cli.md、architecture.md、cli-view-addition.md）
6. 出力 byte-identity の確認（capture pin）

### 落とし穴
- docs-charset: U+2014 / ・ を持ち込まない（cli.md は U+2015/… が既存慣行）
- NULL 意味の非対称（inventory :2455-2462 vs overhead :2041-2049）は JSON 形
  不変（本 PBI では trend のみ、非対称の統一は後続）
- 確定決定（PBI 32/33/31、cli/views/ 着地）は再開しない
- tripwire（budget 3,383 slack なし）: payload 型は trend.rs / 新 module へ

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとして実装されパスする
- [ ] trend の table/JSON が 1 つの typed payload から導出され、精度規則が
  1 箇所に寄る
- [ ] 最初の golden JSON contract テストが着地する
- [ ] `just check`（直接実行）と `cargo test` が green（byte-identity 込み）
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] cli.md、architecture.md、cli-view-addition.md 更新済み
