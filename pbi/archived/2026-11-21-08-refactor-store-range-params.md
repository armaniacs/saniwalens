# PBI: store の範囲バインド＋LIMIT 組立を 1 ヘルパーに統一する

## 優先度
- 順位: 08 / 全14件
- RICEスコア: 6.0（Reach=6 / Impact=0.5 / Confidence=100% / Effort=0.5日）
- 根拠: `_briefing` 系リーダー 9 関数が、`range_clause` の戻り値を
  `Vec<rusqlite::types::Value>` に変換し `saturating_limit(limit)` を push する
  同じ 4 行ブロックを繰り返している。行数ではなく「LIMIT 句の境界契約
  （`saturating_limit` の意味）」が 9 箇所に分散している点が保守上の弱点。
  新規 briefing リーダーを追加するたびに同一ブロックのコピペが増える構造

## ユーザーストーリー
saniwalens のメンテナとして、briefing リーダーを追加するときに範囲バインドと
LIMIT 組立の定型を書かずに済み、LIMIT の境界契約がコード上 1 箇所だけに
存在する状態がほしい。なぜなら契約が 9 箇所にコピペされていると、
新しいリーダーで片方だけ崩れたときに境界条件のバグが静かに混入するからだ。

## 背景
- `src/store.rs` の `_briefing` 系リーダー 9 箇所が同型の 4 行ブロック
  （`binds.iter().copied().map(Value::from)` + `params.push(Value::from(saturating_limit(limit)))`）
  を繰り返す:
  - `file_edits_briefing` — src/store.rs:1885-1890
  - `work_counts_briefing`（kind バインドを先頭に挟む）— src/store.rs:1922-1925
  - `error_tool_breakdown_briefing` — src/store.rs:1951-1956
  - `error_counts_by_project_briefing` — src/store.rs:1986-1991
  - `prompt_behavior_counts_briefing` — src/store.rs:2017-2022
  - `skill_usage_briefing` — src/store.rs:2035-2040
  - `subagent_by_agent_briefing` — src/store.rs:2073-2078
  - `skill_event_costs_briefing` — src/store.rs:2106-2111
  - `effective_catalog_briefing` — src/store.rs:2138-2143
- 契約のもととなるヘルパーは既に 1 箇所に定義されている:
  `saturating_limit`（src/store.rs:577-579。`usize` が `i64` より広い
  プラットフォームで wrap ではなく飽和させる）、`range_clause`
  （src/store.rs:587-603。`(SQL 断片, Vec<i64> バインド)` を返す）。
  分散しているのはこれらの「呼び出し側の組立」
- 非 briefing 側にも同型の variant がある: `work_counts`
  （src/store.rs:1187-1189）は LIMIT 無しで kind バインドを先頭に挟む
- 改善案: `fn range_params(binds: &[rusqlite::types::Value], extra: impl IntoIterator<Item = rusqlite::types::Value>) -> Vec<rusqlite::types::Value>`
  のような小ヘルパーを 1 つ足し、9 箇所を 1 行呼び出しに置換する
  （`saturating_limit` の呼び出しはヘルパー内に統一）。
  SQL 文字列・クエリ結果は一切変えない

## BDD受け入れシナリオ

```gherkin
Scenario: briefing 系リーダーの振る舞いは変わらない
  Given 範囲と limit を受け取る 9 つの _briefing 系リーダーがある
  When  ヘルパー統一のリファクタを適用する
  Then  store 系テストがすべて green であり、返される行・順序・件数が変わらない
  And   各リーダーの SQL 文字列はリファクタ前と同一である

Scenario: LIMIT の境界契約はヘルパー内に一元される
  Given saturating_limit は LIMIT バインドを飽和させる境界契約である
  When  9 つの _briefing 系リーダーがパラメータを組み立てるとき
  Then  saturating_limit の呼び出しはヘルパー range_params の内部にのみ現れる
  And   個々のリーダー本体からの直接呼び出しは消えている

Scenario: kind バインドを先頭に挟む variant も 1 行で書ける
  Given work_counts_briefing は kind セルを最初のバインドとして要求する
  When  同リーダーがヘルパーでパラメータを組み立てるとき
  Then  先頭に kind セル、続いて範囲バインド、末尾に LIMIT が並ぶ
  And   クエリ結果はリファクタ前と同一である
```

## 受け入れ基準
- [x] `src/store.rs` の 9 箇所の `_briefing` 系リーダーが、パラメータ組立を
  1 行のヘルパー呼び出しに置換している
- [x] `saturating_limit` の呼び出しがヘルパー内に統一され、リーダー本体からの
  直接呼び出しが消えている
- [x] ヘルパーは先頭追加バインド（`work_counts_briefing` の kind セル）を
  `extra` で受けられる
- [x] SQL 文字列・クエリ結果が一切変わっていない（リファクタ前後の diff 空で確認）
- [x] `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、
  `cargo test` が green

## テスト戦略
- 単体: ヘルパー自体の境界テスト（空 binds + limit のみ、binds あり、
  extra あり）。返す Vec の要素順（binds → extra → LIMIT）を固定する。
  ただし本 PBI は振る舞い不変リファクタであり、既存の store 系テストが
  等価性の証人になるため、新規テストはヘルパーの契約固定に最小限にとどめる
- 統合: 既存の store 系テスト全 green。`_briefing` 系リーダーの返す行・
  順序・LIMIT 挙動が変わらないことを既存テストで確認する
- E2E: `cargo test` スイート全体に加え、リファクタ前後で `format!` される
  SQL 文字列に 1 バイトの差分も無いことを確認する（振る舞い不変の判定線）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- **着手順序（重要）**: 本 PBI は `src/store.rs` に触れる。同日の PBI 05
  （row 型移管、`pbi/2026-11-21-05-refactor-core-row-ownership.md`）が
  先行バッチで `store.rs` を触るため、**05 の完了後に着手すること**
  （バックログの依存マップ: 01 → 05 → 08。store.rs 排他）
- ヘルパー名・シグネチャは背景に示した `range_params` を出発点とする。
  `range_clause` の戻りは `Vec<i64>`（src/store.rs:587）なので、`Value` 変換を
  ヘルパー内に寄せるか呼び出し側に残すかは実装時に合わせる。
  要件は「9 箇所が 1 行になり、`saturating_limit` がヘルパー内にだけ現れる」こと
- `work_counts_briefing`（src/store.rs:1922-1925）のみ kind セルを先頭に
  置くため、`extra`（先頭に差し込む追加バインド）経由で置き換える
- 非 briefing の `work_counts`（src/store.rs:1187-1189）は LIMIT を組まない
  同型変種。本 PBI のスコープは LIMIT 組立を持つ 9 箇所であり、これを
  変える必要はない
- SQL 文字列は一切変えない。`format!` 内の文字列リテラルに触れたら
  振る舞い不変の前提が崩れるため、既存テストの green と文字列 diff 空を
  崩さないこと

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] store 系テスト全 green かつ SQL 文字列 diff 空
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo test` が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
