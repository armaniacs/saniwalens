# PBI: 窓付き read の flag 語彙を型に写し doctor の format を open 前に判定する

## ユーザーストーリー
利用者として、`doctor --format markdown` が store を開く前に明確なエラーで拒否されて
ほしい、なぜなら現状は analyze と freshness 行が走り、db が無ければ作成された後に
「markdown は無い」と言われるからだ。保守担当者としては、scope を持たない view が
未読の filter を受け取らない型になってほしい。

## 優先度
- 順位: 3 / 全候補数 10
- RICEスコア: 8.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.5日）
- 根拠: 現在の spec 違反（doctor の format 判定順）を閉じつつ、view ごとの flag 語彙を
  戻り型に写す。⑩（views tripwire）と⑪（scratch guard）の前提として先行する。

## ビジネス価値
- 窓付き view の flag 語彙（scope の有無、format の値域）が型に現れ、view 追加時の
  静かな取りこぼしを compiler とテストが捕まえる。
- `doctor --format markdown` が store open / analyze / freshness / db 作成の前に失敗する。
- 測定方法: `--scope` の e2e が実際に行を変えること、doctor の markdown 失敗時に
  stderr に freshness 行が無く db も作られないこと、9 view の help が byte 同一であること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 7本）
- `src/cli/window.rs:71-76` の `WindowedRead` は `sess` / `filter` / `format` / `window`
  の 4-field bag。scope を持たない usage/overhead/prompts/stuck は `scope=None` で
  `filter` を受け取るが読まない。
- `--format` の宣言は 11 箇所・文面 4 種。同一文面は 7 view（usage/inventory/waste/
  overhead/prompts/failures/stuck）で、doctor は `table (human text) | json (the routed
  findings)`、trend は `table | markdown | json`。候補文の「8」は誤り。
- `doctor --format markdown` は `dispatch.open` の後、`doctor()` 内 :930 で bail する。
  非 frozen なら analyze が走り、freshness 行が出て、db が無ければ作成される。
  `docs/specs/cli.md:74-80` の「flags before store opens」に違反。テストは 0 件。
- `--scope` を argv に渡す e2e テストも 0 件。inventory が scope を落としても全テスト緑。
- 根本原因: flag 語彙が型にも spec の機械可読な表にも無く、11 箇所の複製 doc と
  arm の `None` 引数にだけ宿る。`open` が 4-field bag に潰した時点で view 差が
  compiler からもテストからも見えなくなる。

## BDD受け入れシナリオ

```gherkin
Scenario: scope を持つ view だけが filter を受け取る
  Given WindowedRead から filter を外し ScopedRead を分離する
  When 4 つの unscoped view と 4 つの scoped view を配線する
  Then unscoped は sess/format/window だけを、scoped は filter も受け取る

Scenario: doctor の markdown は store を開く前に拒否される
  Given db が存在しない
  When `saniwalens doctor --format markdown` を実行する
  Then "unknown --format" で失敗し、freshness 行が出ず、db も作成されない

Scenario: scope は実際に出力を変える
  Given global と project の findings を持つ合成 store がある
  When `inventory --scope global` と `--scope project` を実行する
  Then 行の集合が異なる

Scenario: help はバイト同一である
  Given 9 view の capture がある
  When 本 PBI を着地させる
  Then 9 view の --help 出力は capture と一致する
```

## 受け入れ基準
- [ ] `WindowedRead` を `sess` / `format` / `window` に痩せさせ、
  `ScopedRead { read: WindowedRead, filter: ScopeFilter }` を追加する
- [ ] `open`（scope 無し）と `open_scoped`（scope -> format -> period -> store の順）に
  分ける。順序は module 内で構成的に固定し、arm の行順に戻さない
- [ ] mod.rs の 4 unscoped arm は `open`、4 scoped arm は `ScopedRead` の分割代入にする
- [ ] `ViewFormat`（`TableMarkdownJson` / `TableJson`）を report.rs の `Format` の隣に置き、
  doctor 専用の `open_scoped_table_json` 入口で store open 前に判定する
- [ ] doctor の markdown 失敗は `unknown --format 'markdown' (table|json)` で、
  analyze / freshness / db 作成の前に起きる
- [ ] `--scope` の e2e を 1 本、doctor markdown の失敗順序 e2e を 1 本追加する
- [ ] 9 view の `--help` は byte 同一（`tests/cli_help_golden.rs` を更新しない）
- [ ] `src/cli/mod.rs` の行数は ±0。動く場合は tripwire budget を spec 更新と同一 PR で
  再測定する（zero-slack）
- [ ] spec 更新: cli.md の open 列挙と doctor の値域、architecture.md の report 行。
  CHANGELOG は履歴ファイルのため更新しない
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `inventory --scope global` と `--scope project` の出力差
- `doctor --format markdown` が db を作らず freshness も出さないこと

### 統合テスト
- 既存の `the_windowed_open_resolves_flags_in_the_arms_order` を unscoped/scoped の
  2 本に分割し、同じ順序 assert を維持
- `open_scoped(None, ...)` が `ScopeFilter::All` を返すこと

### 単体テスト
- `ViewFormat` の受理値（table/json のみ）とエラー文言
- 7 view + trend は table/markdown/json を受理し、doctor は markdown を拒む

## 実装アプローチ
- **Outside-In**: `--scope` の e2e と doctor markdown の e2e を先に赤にし、型分離 ->
  format 語彙 -> 配線 -> spec の順。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
1.5 SP（E=1.5日。RICE 8.0）

## 技術的考慮事項
- 依存関係: ④ -> ⑩、④ -> ⑪（`src/cli/window.rs` の同一ファイル）
- `FormatArgs` group は本 PBI では作らない。7 view の文面は実際には json も受けるのに
  `table|markdown` と書いており、group 化は嘘を 1 本に固める。文面修正と同一 PR の
  別 PBI に送る
- clap の value_parser で doctor を縛る案は、値域の機構が 2 本になり終了コードも
  変わるため弃却。parse 段の 1 箇所に寄せる
- 非機能要件: help byte-identity、エラー順序、プライバシー（合成 store のみ）

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `src/cli/window.rs`: `WindowedRead` :71-76、`open` :105-133、`open_pair` :137-160、
  順序 pin :587-664、`SINGLE_WINDOW_VIEWS` :425-434。
- `src/cli/mod.rs`: dispatch :288-377、doctor の markdown bail :930、
  `parse_format` の呼び出しは window.rs。
- `src/cli/report.rs`: `Format` と `parse_format`。
- `docs/specs/cli.md:74-80` の flags-before-store 契約、:145-155 の format 節。
- `src/session.rs:143-147` の freshness、:192-200 の refresh eprintln。

### 実装手順
1. Red: `--scope` の e2e と doctor markdown の e2e を書く
2. `ScopedRead` と `open_scoped` を追加し、4+4 arm を配線
3. `ViewFormat` と doctor 専用入口を追加し、bail を parse 段へ移す
4. 順序 pin を 2 本に分割
5. spec を更新（CHANGELOG は不変）
6. 検証（help golden、行数、フルスイート）

### 落とし穴
- `open_scoped` が `parse_scope -> open` に委譲すると、scope が format に勝つ順序が
  構成的に固定される。arm 側で parse_scope を呼ばない
- mod.rs の行数を増やさない。引数で語彙を渡す形は rustfmt 折返しで +5 行になる
- doctor の markdown 文言を変える場合は spec に記録する（CHANGELOG は不変）。help は変えない
- `--format md` の alias は現状受理されない。挙動を変えない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] unscoped view が filter を受け取らない型になっている
- [ ] doctor の markdown が store open 前に失敗する
- [ ] help 9 view が byte 同一、mod.rs 行数 ±0
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] cli.md、architecture.md 更新済み（CHANGELOG は不変）
