# PBI: --scope の e2e を 3 view に広げ、session だけの previous window を trend で pin する

## ユーザーストーリー
保守担当として、`--scope` が waste / failures / doctor の行を実際に変えることと、
前の窓に session だけがある trend の note を e2e で見たい、なぜなら `--scope` の e2e は
inventory だけで filter を `All` に固定しても緑、trend の note も「event が 1 つも
無い」形しか無いからだ。

## 優先度
- 順位: 7 / 全候補数 9
- RICEスコア: 2.7（R=2 / I=1 / C=2 / Effort=1.5日）
- 根拠: 検出力の穴（変異を落とせない）を埋める作業で、製品の振る舞いは変えない。
  I=1 は「壊れても利用者の見え方は変わらず、テストの網が薄いだけ」の意。E=1.5日 の
  大半は scoped view 用の合成 store（sessions / events / surfaces）の用意。

## ビジネス価値
- `--scope` の配線が 4 view すべてで pin され、filter の取り違えが CI で落ちる。
- trend の note が「session はあるが event が無い」境界でも正しく出ることが固定され、
  PBI 46 の measured-zero との区別が e2e で守られる。
- 測定方法: waste の filter を `All` に固定すると赤。`previous_events_are_empty` を
  `has_no_baseline` に置き換えると赤。

## 背景（2026-11-02、round4 最終 whole-branch review M10 / F1 の呼び出し側）
- `tests/windowed_flag_vocabulary.rs:107-157` の
  `the_scope_flag_changes_the_rows_inventory_reports` は inventory の id 列だけを
  比較する。`--scope` を受ける view は inventory / waste / failures / doctor の 4 つ
  （`src/cli/mod.rs:296-382` の dispatch が `open_scoped` / `open_scoped_table_json`
  を通る）。waste / failures / doctor の filter を `All` に固定しても既存の 544 本は
  緑のまま。
- その理由は fixture にある。`write_store`（`:57-72`）は surfaces を 2 行書くだけで、
  sessions も events も無い。waste は「使われていない高コスト surface」、failures は
  失敗 tool_use、doctor は routed findings なので、events / sessions が無いと scope で
  差が出る行が作れない。
- trend 側: `src/cli/trend.rs:193-201` の `previous_events_are_empty` が note の判断で、
  文言は `:705-712`。e2e は「前の窓に event が無い」（`tests/trend_markdown.rs:142-158`）
  と「measured zero」（`:219-254`）の 2 形だけで、「前の窓に session 行はあるが event が
  無い」形が無い。この形は `has_no_baseline`（`:164-190`）と totals 軸の分岐
  （`:371-401`）が同時に効く境界で、note と totals の扱いが食い違い得る。
- 根本原因: 旗の語彙（どの view がどの値を取るか）は PBI 45 で e2e 化したが、旗の
  効果（scope が行を変える）は 1 view だけだった。

## BDD受け入れシナリオ

```gherkin
Scenario: scoped view が scope で行を変える
  Given global と project demo の 2 層を持つ合成 store
  When waste / failures / doctor を --scope global と --scope project で実行する
  Then 行集合が両方向に異なる
  And global は project の行を含まず、project は global の行を含まない

Scenario: 前の窓に session だけがある trend
  Given 前の窓に session 行があり event が 0、現在の窓に event がある
  When trend --format markdown を実行する
  Then note が previous window の event 不在を告げる
  And totals の sessions 行は比較を主張し、kind 行は n/a になる

Scenario: filter を All に固定した変異で赤
  Given waste の scope filter を All に固定する
  When scoped e2e を回す
  Then 赤になる
```

## 受け入れ基準
- [ ] `tests/windowed_flag_vocabulary.rs` に、`--scope` を持つ view を clap から導出
  する helper を追加する（`get_arguments()` に `--scope` があるかで filter）。既存の
  `table_markdown_views()`（`:28-42`）と同じ導出流儀にし、view 名を手で並べない
- [ ] inventory 以外の 3 view で row 差分を assert する。比較は view ごとの行抽出
  helper（inventory の `inventory_ids`（`:92-105`）に相当）で行い、`--frozen` を付ける
- [ ] 合成 store を surfaces + sessions + events に拡張する。`SessionMeta` /
  `IngestBatch` の組み立ては `tests/trend_markdown.rs:14-54` の helper と同型なので、
  重複を避けるなら `tests/common` へ昇格する（PBI 50 の `ScratchDir` とは独立）
- [ ] waste: global に高コスト未使用 surface、project に別の未使用 surface を 1 つずつ
- [ ] failures: 同じ失敗 tool_use を global 側と project 側に 1 つずつ
- [ ] doctor: `--format markdown` は拒否されるので `table` で比較する。routed 行
  （global セクション / project セクション）が scope で入れ替わることを assert
- [ ] `tests/trend_markdown.rs` に
  `trend_note_fires_when_the_previous_window_holds_sessions_but_no_events` を追加。
  helper `write_session_only_store`（session を ingest し events を空に）を足す
- [ ] note の文言と、totals の `sessions` 行が `n/a` でないことを両方 assert する
  （片方だけだと filter の取り違えを見逃す）
- [ ] 変異確認を PR 本文に記録: waste の filter を `All` に固定 -> 赤 /
  `previous_events_are_empty` を `has_no_baseline` に置換 -> 赤
- [ ] fmt、clippy、対象テスト、フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 4 view（inventory は既存）で `--scope global` / `--scope project` の行集合を比較
- trend の session-only previous window を markdown で比較

### 統合テスト
- なし（binary の stdout を直接見る e2e で足りる）

### 単体テスト
- `src/cli/trend.rs` の test module に「session のみ previous」の payload 1 本。e2e が
  落ちたときに、payload か描画かを切り分けるため
- 行抽出 helper の自己テスト（ヘッダ名で列を探す。列が入れ替わったら赤）

## 実装アプローチ
- **Outside-In**: 各 view の scope e2e を書き、`All` 固定の変異で赤を確認してから
  fixture を整える -> trend の session-only e2e -> payload 単体。実装は
  subagent-driven-development で行う。

## 見積もり
1.5 SP（E=1.5日。RICE 2.7）

## 技術的考慮事項
- 依存関係: PBI 45 の着地後（`--scope` e2e の土台）。`tests/common` を触るので PBI 50
  の `ScratchDir` と衝突しないよう、helper は別ファイルか追記で足す
- doctor の期待は spec から導く。`docs/specs/cli.md` の Scope 節を先に読み、scope が
  routed セクションをどう変える設計かを確認してから assert を書く（テストが設計を
  決めない）
- `--scope project` は全 project の合算、`project:<slug>` は 1 つ。差が出る形にする
  ため project は 1 つに絞る
- 非機能要件: フィクスチャは合成のみ。実データを読まない（session-data-privacy）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `tests/windowed_flag_vocabulary.rs:28-42` `table_markdown_views`、`:57-72`
  `write_store`、`:74-90` `run`、`:92-105` `inventory_ids`、`:107-157` scope の
  inventory e2e、`:159-186` doctor の markdown 拒否、`:188-225` store open 前の拒否。
- `src/cli/mod.rs:296-382` の dispatch（`open_scoped` / `open_scoped_table_json`）。
- `src/cli/trend.rs:164-190` `has_no_baseline`、`:193-201`
  `previous_events_are_empty`、`:371-401` totals 軸、`:705-712` note。
- `tests/trend_markdown.rs:14` / `:32` / `:55` の helper、`:86` `trend_output`、
  `:142-158` note の e2e、`:219-254` measured zero。
- 既実装の確認: `--scope` の e2e は inventory の 1 本だけ。trend の note e2e は 2 形。
  よって本 PBI は「既存の穴を埋める」作業で、製品コードは変えない（変異で赤くなる
  ことの確認が本体）。

### 実装手順
1. `--scope` を持つ view の導出 helper と、view ごとの行抽出 helper
2. fixture を surfaces + sessions + events に拡張
3. waste / failures / doctor の e2e を書き、変異（`All` 固定）で赤を確認
4. trend の session-only e2e と helper、payload 単体
5. 変異 2 種の赤を記録
6. 検証: 対象テスト、フルスイート、fmt、clippy

### 落とし穴
- waste は `static_tokens` が 0 だと行が出ない。surface helper（`:44-55`）の 40 tokens
  を維持する
- doctor は markdown を拒むので、他の view と同じ markdown 比較にしない
- `--frozen` を忘れると analyze が走り、合成 home の外を読み得る（既存 e2e はすべて
  付けている）
- trend の note は table と markdown の両方に出るが、framing（preamble）は table だけ。
  markdown で比較すると note だけを見ることになる
- `write_session_only_store` は `IngestBatch::new(&meta, &[], &[])` で events を空に
  できる（`tests/trend_markdown.rs:42-50` の `work_events` を付けない形）
- 両方向の assert（global に project の行が無い / その逆）を省くと、filter が
  「全部返す」変異を落とせない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 4 view すべてで scope の行差分が pin されている
- [ ] trend の session-only previous window の note が e2e で pin されている
- [ ] 変異 2 種で赤になることを PR 本文に記録
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
