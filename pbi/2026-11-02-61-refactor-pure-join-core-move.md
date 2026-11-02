# PBI: trend の row/join 型の所有を決めて core へ移す

## ユーザーストーリー
保守担当として、trend の two-window join とその row 型が core にあり、store には
query だけが残ってほしい、なぜなら placement rule の 1st pattern が `src/store.rs` に
残り、`cli/trend.rs` が store の名前空間から分析型を import し続けているからだ。

## 優先度
- 順位: 9 / 全候補数 9（旧 PBI 61 の再スコープ。trigger は発火済み）
- RICEスコア: 8.0（R=3 / I=2 / C=2 / Effort=1.5日）
- 根拠: 旧 12.0 は E=1日で型所有の判断と 5 点目の prose を数えていなかった。
  判断 0.5 日 + 移動 0.5 日 + tripwire/prose 0.5 日として E=1.5、RICE 8.0。
  順位は最後のまま（他 PBI の着地後に、依存する PBI 53 の上で行う）。

## trigger（旧 PBI の停止条件を解消）
旧 PBI 61 は「次の trend metric/view 着地 PR、または 2 個目の SQLite 非依存 join」を
待つとして blocked にした。rule 着地 `7a95563`（2026-11-01）の後に、trend の metric
contract を変える `1ecda2a`（baseline 型）と `3b4349f`（reason の JSON 配線）が着地して
おり、これは rule の trigger 1 に当たる。ただし move はその着地に同乗しておらず、
本 PBI は trigger 発火後の **follow-up remediation** である（co-landing ではない）。
この missed co-landing を台帳と rule に記録し、trigger 2 は次の accumulation に armed
のまま残す。

## 決定（旧 PBI の自己矛盾の解消）
旧 PBI は「4 型を core へ」「`SkillUsage` / `KindRollup` は store に残す」を同時に
要求していた。しかし `TrendWindowAggregates.skills` は `Vec<SkillUsage>`、
`by_kind` は `Vec<KindRollup>` を取り、`skill_deltas` も `SkillUsage` を引数に取る。
row 型を store に残したままでは core が store を import する逆依存になる。

決定:
- `SkillUsage` / `KindRollup` も `src/core/trend.rs` へ移す。どちらも純データで、
  store が core 型を構築する現在の依存方向（store -> core）に乗る。
- `skill_deltas` の入力契約は変えない（`&[SkillUsage]` のまま）。呼び出し側は
  core の型を見る。
- `cli/trend.rs` の `kind_pairs` も core へ移す。これは列挙外に残っていた 5 個目の
  純 join で、移動後も cli に残すと rule 違反が残る。
- `FIRST_PATTERN_JOINS` は 4 から 5 に増やし、`kind_pairs` を含める。

## BDD受け入れシナリオ

```gherkin
Scenario: 移動後も trend の出力が不変
  Given 移動前の trend の table / markdown / json の capture
  When 移動を着地させる
  Then JSON capture は byte 同一、markdown / 表の既存出力テストは green
  And tests/trend_markdown.rs と trend_json_golden が green

Scenario: 移動が tripwire を反転させる
  Given 5 つの join が core にあり store に無い
  When cargo test --test pure_join_placement を回す
  Then move side は core 在 + store 不在を assert し、5 エントリは空でない
  And rule は移動先を名指す

Scenario: core が store を import しない
  Given src/core/trend.rs と src/cli/trend.rs
  When cargo test --test pure_join_placement を回す
  Then core に crate::store への参照が無い
  And cli/trend.rs は store の分析型を import しない
```

## 受け入れ基準
- [ ] `src/core/trend.rs` を新設し、`SkillUsage` / `KindRollup` / `TwoWindow<T>` /
  `TrendWindowAggregates` / `SkillDelta` / `skill_deltas` / `kind_pairs` を定義する。
  `src/core.rs` に `pub mod trend;` を足す
- [ ] `src/store.rs` から 5 型 + 1 関数（`SkillUsage` / `KindRollup` / `TwoWindow` /
  `TrendWindowAggregates` / `SkillDelta` / `skill_deltas`）の定義を削除し、
  `crate::core::trend` から import する。`kind_pairs` は `cli/trend.rs` から移す。
  `Store::trend_aggregates` / `trend_window_aggregates` の戻り値は core の型にする
- [ ] `src/cli/trend.rs` の import を `crate::core::trend` に張り替え、`kind_pairs` の
  定義を削除して core から import する。`crate::store` の分析型 import を残さない
- [ ] `skill_deltas_order_by_the_largest_absolute_delta` を core の module へ移す。
  `two_window_aggregation_splits_events_between_the_windows` は store に残す
- [ ] `tests/pure_join_placement.rs`:
  (a) `FIRST_PATTERN_JOINS` を 5 エントリにし、`kind_pairs` を足す
  (b) `the_enumerated_joins_still_sit_in_store_rs` を「core に在り、store に無い」へ
  反転する
  (c) `rule.contains("src/store.rs")` の単純 contains をやめ、rule が移動先
  （`core/trend.rs` または `core`）を名指すことを assert する
  (d) 外部アンカーを 2 本足す: `src/core/trend.rs` が `crate::store` を import しない、
  `src/cli/trend.rs` が `crate::store` の分析型を import しない
  (e) `docs/specs/architecture.md` の「still sit in `store.rs`」が消えていることを
  assert する（prose の外部 pin）
- [ ] 4 点を同一 PR で更新する:
  (a) `.claude/rules/pure-join-placement.md` の Rules と「The 1st pattern, named」を
  core 在 + store 不在へ、(b) `docs/specs/architecture.md:99` の core row と
  `:139-152` の段落、(c) 台帳の deferral note を「移動済み（日付・PR）」へ、
  (d) tripwire の move side
- [ ] `docs/specs/cli.md` に trend の型が store 由来と読める記述があれば core に直す。
  現状は型名を出していないため変更不要であることを確認して PR に記録する
- [ ] trend の JSON が byte 同一、markdown / 表の既存出力テストが green、fmt、
  clippy、フルスイートが green
- [ ] 旧 PBI 61 の「単独着手しない」「trigger 待ち」の記述を、trigger 発火済みの
  本再スコープに置き換える。台帳の RICE と順位も更新する

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `tests/trend_json_golden.rs` の byte 比較
- `tests/trend_markdown.rs` の markdown / 表の出力
- `tests/cli_help_golden.rs` の trend help が不変

### 統合テスト
- `tests/pure_join_placement.rs` の 5 エントリ、move side、集合一致、MOVE_REMEDY、
  外部アンカー 2 本、architecture の stale 不在
- `Store::trend_aggregates` / `trend_window_aggregates` が core 型を返すこと
  （コンパイル + 既存 store の trend test）

### 単体テスト
- `skill_deltas` の順序 test が core 側で DB 無しに走る
- `kind_pairs` の「片側だけの kind は None」test を core に置く
- `TwoWindow` / `TrendWindowAggregates` / `SkillDelta` の derive が移動で欠けない
  こと（コンパイルで担保）

## 実装アプローチ
- **Outside-In**: まず tripwire の反転と外部アンカーを書いて赤を確認する ->
  `src/core/trend.rs` に型と関数を移す -> store / cli の import と戻り型を更新 ->
  4 点の prose と台帳 -> golden の byte 比較 -> フルスイート。
- 移動は「store から定義を消す」ことを完了条件にする。再輸出だけの空 module は
  墓標コメントと同じで不可。

## 見積もり
1.5 SP（E=1.5日。RICE 8.0）

## 技術的考慮事項
- 依存方向: `store -> core` は既存（`src/store.rs:14-19`）。`core -> store` は 0 件で
  あり、移動後も 0 を保つ。これを外部アンカーで pin する
- `SkillUsage` / `KindRollup` を移すと `tests/pure_join_placement.rs:152-167` の
  集合一致は壊れない（rule と `FIRST_PATTERN_JOINS` は join 記号だけを列挙し、
  row 型は含まない）。旧 PBI の「壊れる」は誤り
- `kind_pairs` を 5 個目として足すと、集合一致 test の「正しい追加」と「違反」を
  区別できない問題が残る。追加は rule と定数の同時更新を同一 PR で行い、外部
  アンカー（core 在 / store 不在 / cli の store import 不在）で補う
- `cli/trend.rs` の `kind_pairs` は `TwoWindow<TrendWindowAggregates>` を取る。
  core へ移すと cli 側の import が減る
- 台帳の deferral note は削除しない。「移動済み」に書き換える（履歴として残す）
- 非機能要件: なし（移動のみ、外部契約は不変）

## 実装者向け注記

### 現状コード（2026-11-02、HEAD 069e112）
- `src/store.rs:322` `SkillUsage`、`:331` `KindRollup`、`:342` `TwoWindow`、`:350`
  `TrendWindowAggregates`、`:361` `SkillDelta`、`:372-397` `skill_deltas`。
- `src/store.rs:1286-1301` の `trend_aggregates` / `trend_window_aggregates`。
- `src/store.rs:3647` の純粋 test、`:3678` の store 例外 test、`:3793` の
  `skill_deltas` 呼び出し。
- `src/cli/trend.rs:13` の import、`:423-443` の `kind_pairs`、`:294` / `:348` /
  `:356` / `:424` / `:525` の使用。
- `tests/pure_join_placement.rs:32-37` の 4 エントリ、`:41-66` MOVE_REMEDY、
  `:105-124` の rule pin、`:128-137` move side、`:152-167` 集合一致、
  `:170-209` 4 点 pin、`:212-224` architecture。
- `docs/specs/architecture.md:99` の core row、`:132-152` の段落。
- `.claude/rules/pure-join-placement.md` の Rules と「The 1st pattern, named」。
- 台帳 `pbi/2026-10-30-00-backlog.md:214-223` の deferral note。

### 実装手順
1. `src/core/trend.rs` を作り、型と関数を移す。`src/core.rs` に module を足す
2. store の定義を削除し、core から import。query の戻り型を core 型にする
3. cli/trend.rs の import と `kind_pairs` を移す
4. tripwire を 5 エントリ + 反転 + 外部アンカー 2 本 + architecture stale 不在に
   作り直す
5. rule / architecture / 台帳 / PBI 61 を更新
6. `cargo test --test pure_join_placement`、trend の JSON golden と markdown / 表の
   出力テスト、フルスイート、fmt、clippy
7. 変異確認: `core/trend.rs` から `kind_pairs` を消す -> tripwire 赤、
   `cli/trend.rs` に `crate::store` の分析型 import を戻す -> 外部アンカー赤、
   `architecture.md` に「still sit in `store.rs`」を戻す -> prose pin 赤

### 落とし穴
- `FIRST_PATTERN_JOINS` の型は `[(&str, &str); 4]`。5 に変えると定数型も変わる。
  空化・エントリ削除・墓標コメントで充足しない
- `rule.contains("src/store.rs")` の単純 assert は、rule の trigger 段落に
  `src/store.rs` が残るため移動後も真になる。移動先の `core` を名指す assert に
  置き換える
- `SkillUsage` / `KindRollup` を動かすと `src/store.rs` の integration test の
  import が要る（`:3793`）。`tests/pure_join_placement.rs` の集合一致は壊れない
- trend の JSON は `cli/trend.rs` が payload を組み立てる。型を移しても出力の
  フィールド順が変わらないことを golden の byte 比較で確認する
- `kind_pairs` を core に移すと、`src/cli/trend.rs` の private test が
  `super::kind_pairs` を呼んでいる場合は import を足す
- 台帳の note は削除せず、移動済みの日付と PR を追記する
