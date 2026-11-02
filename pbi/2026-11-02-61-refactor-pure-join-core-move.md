# PBI: pure-join の trend 4 型を core へ移す（trigger 乗車・単独着手しない）

## ユーザーストーリー
保守担当として、trend の two-window join が core にあり、store には query だけが残って
ほしい、なぜなら placement rule の 1st pattern が `src/store.rs` に残り、`cli/trend.rs`
が store の名前空間から分析型を import し続けているからだ。ただし移動は trigger（次の
trend metric/view 着地 PR、または 2 個目の SQLite 非依存関数）に乗せる。

## 優先度
- 順位: 9 / 全候補数 9（trigger 待ち。スコアは最高だが単独着手しない）
- RICEスコア: 12.0（R=3 / I=2 / C=2 / Effort=1日）
- 根拠: 12.0 は本ラウンド最高だが、rule が「次の trend metric/view 着地 PR に同乗」と
  定めており、trigger の無い単独 PR は契約違反になる。よって実施順は最後に置き、
  トリガーが来た PR に同乗させる。E=1日 は移動 0.25 + tripwire 反転 0.25 + spec/台帳
  0.25 + 検証 0.25。

## ビジネス価値
- `store.rs` が query と row hand-off に戻り、分析ロジックの単体テストが DB 無しで
  走る（rule が求める層の帰属）。
- 移動の 4 点が同じ PR で揃い、tripwire が「store 在」から「core 在 + store 不在」へ
  反転する。次に store へ純粋関数が生えたら同じ義務が再発火する。
- 測定方法: `tests/pure_join_placement.rs` が反転後も green、trend の golden 3 本
  （markdown / json / 表）が byte 同一、`rg -n "pub fn skill_deltas" src/store.rs` が 0。

## 背景（2026-11-02、PBI 41 / 51 の deferral note の実行）
- 定義はまだ store にある: `src/store.rs:342` `TwoWindow<T>`、`:350`
  `TrendWindowAggregates`、`:361` `SkillDelta`、`:372` `skill_deltas`。消費者は
  `src/cli/trend.rs:13` の 1 行（`use crate::store::{KindRollup,
  TrendWindowAggregates, TwoWindow, skill_deltas};`）。
- query 側は store に残る: `Store::trend_aggregates`（`:1286`）と
  `trend_window_aggregates`（`:1293`）。これらは row hand-off で、rule の「store は
  query と hand-off を持つ」に適合する。
- テスト: `skill_deltas_order_by_the_largest_absolute_delta`（`:3647`）は純粋関数の
  test なので関数と共に移る。`two_window_aggregation_splits_events_between_the_windows`
  （`:3678`）は rule が明記する store 例外として残る。
- tripwire: `tests/pure_join_placement.rs:32-39` の 4 エントリ、`:105-119`
  （`:112` が `rule.contains("src/store.rs")`）、`:128-137`（move side）、
  `:152-168`（集合一致）、`:41-66`（MOVE_REMEDY の 4 点）。
- 4 点契約の行き先: `.claude/rules/pure-join-placement.md`（Rules と
  「The 1st pattern, named」）、`docs/specs/architecture.md:99`（core row）と
  `:139-152`（段落）、台帳の deferral note（`pbi/2026-10-30-00-backlog.md` 214-223
  行）、tripwire 自身。`.claude/rules/spec-sync.md:28` は PBI 53 で位置主張を外すので、
  移動 PR は spec-sync を触らない。
- 根本原因: 移動の契約（trigger・4 点・反転）は整ったが、trigger がまだ発火して
  いない。本 PBI はその発火を待つ作業として台帳に残す。

## BDD受け入れシナリオ

```gherkin
Scenario: 移動後も trend の出力が不変
  Given 移動前の trend の table / markdown / json の capture
  When 移動を着地させる
  Then 3 つの capture が byte 同一
  And tests/trend_markdown.rs と trend_json_golden が green

Scenario: 移動が tripwire を反転させる
  Given 4 型が core にあり store に無い
  When cargo test --test pure_join_placement を回す
  Then move side は core 在 + store 不在を assert し、4 エントリは空でない
  And rule は移動先を名指す

Scenario: trigger が無ければ着手しない
  Given trigger が発火していない
  When 本 PBI に着手しようとする
  Then 台帳の deferral note が trigger 未発火を示し、PR を作らない
```

## 受け入れ基準
- [ ] trigger を PR 本文に記録する（乗った trend metric/view 着地 PR の名前、または
  store に現れた 2 個目の SQLite 非依存関数）
- [ ] 4 型と `skill_deltas` を `src/core/trend.rs`（新 module）へ移す。代替は
  `src/core/usage.rs`。`SkillUsage`（`src/store.rs:322`）と `KindRollup`（`:331`）は
  query の row 型なので store に残す（判断を PR 本文に記録）
- [ ] `src/cli/trend.rs:13` の import を `crate::core::trend` に張り替える
- [ ] `Store::trend_aggregates` / `trend_window_aggregates`（`:1286` / `:1293`）は
  core の型を返す（store から core への依存は既存の方向）
- [ ] 単体 test `skill_deltas_order_by_the_largest_absolute_delta`（`:3647`）を core の
  module へ移す。`two_window_aggregation_splits_events_between_the_windows`（`:3678`）
  は store に残す
- [ ] 4 点を同一 PR で:
  (a) `.claude/rules/pure-join-placement.md` の Rules と「The 1st pattern, named」を
  移動済みに（「core 在」+ 「store 不在」）、(b) `docs/specs/architecture.md:99` の
  core row、(c) 同 `:139-152` の「still sit in `store.rs`」を移動済みに、(d) 台帳の
  deferral note（214-223 行）を「移動済み（日付・PR）」に書き換える（削除しない）、
  (e) tripwire の move side
- [ ] tripwire の反転は両方向: `the_enumerated_joins_still_sit_in_store_rs` を
  「core に在り、store に無い」に変え、`:112` の `rule.contains("src/store.rs")` も
  移動先を指すよう反転する（ここを忘れると移動後に赤のまま）
- [ ] `FIRST_PATTERN_JOINS` は 4 エントリのまま。空化・エントリ削除・墓標コメントで
  充足しない（rule の禁止事項）
- [ ] `docs/specs/cli.md` に trend の型が store 由来と読める記述があれば core に直す
  （現状は型名を出していないので、変更不要であることを確認して PR に記録）
- [ ] trend の golden 3 本が byte 同一、fmt、clippy、フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `tests/cli_help_golden.rs` の trend の help が不変（移動は help を変えない）
- `tests/trend_json_golden.rs` / `tests/trend_markdown.rs` が byte 同一

### 統合テスト
- `tests/pure_join_placement.rs` の反転 4 本: rule の名指し、move side、空化禁止、
  集合一致。加えて MOVE_REMEDY の 4 点 pin
- `tests/trend_markdown.rs` の n/a 理由と note が不変

### 単体テスト
- `skill_deltas` の順序 test が core 側で green（DB 無しで走る）
- `TwoWindow` / `TrendWindowAggregates` / `SkillDelta` の derive（Debug / Clone /
  PartialEq / Serialize）が移動で欠けないこと（コンパイルで担保）

## 実装アプローチ
- **Outside-In**: trigger の PR に同乗する形で、まず tripwire の反転を書いて赤を確認
  -> 型と関数を移動 -> import 張り替え -> store の query を core 型に -> 4 点の prose
  と台帳 -> golden の byte 比較。実装は subagent-driven-development で行う。

## 見積もり
1 SP（E=1日。RICE 12.0）

## 技術的考慮事項
- 依存関係: trigger（次の trend metric/view 着地 PR、または 2 個目の SQLite 非依存
  関数）と PBI 53（spec-sync から位置主張が消えていること）。53 が未実施なら move PR
  は 5 点目を直すことになり、53 の仕事を先送りするだけ
- RICE 12.0 でも順位は最後。rule が単独 PR を禁じており、trigger の無い移動は
  「契約を満たすための移動」ではなく「契約の回避」になる
- `TwoWindow` は generic だが使っているのは `TrendWindowAggregates` だけ（grep 済み）
- 新しい module を作る場合は `src/core.rs` の `pub mod` 一覧に 1 行足す
- 非機能要件: なし（移動のみ、外部契約は不変）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/store.rs:322` `SkillUsage`、`:331` `KindRollup`、`:342` `TwoWindow`、`:350`
  `TrendWindowAggregates`、`:361` `SkillDelta`、`:372-397` `skill_deltas`。
- `src/store.rs:1286-1301` の `trend_aggregates` / `trend_window_aggregates`。
- `src/store.rs:3647` の純粋 test、`:3678` の store 例外 test、`:3793` の
  `skill_deltas` 呼び出し（integration test 内）。
- `src/cli/trend.rs:13` の import、`:294` / `:348` / `:356` / `:424` / `:525` の使用。
- `tests/pure_join_placement.rs:32-39`、`:41-66`、`:105-119`（`:112`）、`:128-137`、
  `:152-168`、`:170-210`、`:212-224`。
- `docs/specs/architecture.md:99`、`:132-152`。`.claude/rules/pure-join-placement.md`
  の Rules と「The 1st pattern, named」。
- 既実装の確認: 4 型は store にあり、tripwire が「store 在」を assert している
  （移動は未実施）。よって本 PBI はルールどおりの移動で、新機能ではない。

### 実装手順
1. trigger を PR 本文に書く（乗った PR、または 2 個目の関数）
2. tripwire の反転を先に書き、現行で赤（store 在を core 在と主張するため）を確認
3. `src/core/trend.rs` を作り、4 型 + `skill_deltas` + その純粋 test を移す
4. `src/cli/trend.rs` の import と `src/store.rs` の query の戻り型を更新
5. rule の列挙・architecture の core row と段落・台帳の deferral note を更新
6. 検証: `cargo test --test pure_join_placement`、trend の golden 3 本、フルスイート、
   fmt、clippy

### 落とし穴
- `rule.contains("src/store.rs")` の assert（`:112`）を反転し忘れると、移動後に
  「rule がまだ store を名指している」で赤のまま。ここが 4 点の 4 点目
- `SkillUsage` / `KindRollup` を一緒に動かすと、tripwire の集合一致 test
  （`:152-168`）が壊れる（rule の列挙に無い型を動かすため）。動かすなら rule の列挙も
  同時に増やす。推奨は動かさない（query の row 型）
- store の integration test（`:3793`）が `skill_deltas` を呼ぶ。移動後は import を
  足す必要がある
- 台帳の deferral note は削除しない。「移動済み」に書き換える（履歴として残す）
- 空の module を作って再輸出で済ませるのは「墓標コメント」と同じ。store から定義を
  消すこと
- trend の golden は PBI 46 で reason が増えている。移動で JSON のフィールド順が
  変わらないことを byte 比較で確認する（derive の順序に依存）

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 4 型と `skill_deltas` が core にあり、store に無い
- [ ] tripwire が反転し、4 エントリを保ったまま green
- [ ] 4 点（rule / architecture / 台帳 / tripwire）が同一 PR で更新されている
- [ ] trend の golden 3 本が byte 同一
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
