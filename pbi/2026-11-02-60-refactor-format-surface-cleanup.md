# PBI: --format の宣言を 1 つの group に寄せ、7 view の文面を実態に合わせる

## ユーザーストーリー
利用者として、`--format` の help が実際に受け付ける値を全部書いていてほしい、なぜなら
7 view は「table | markdown」と表示するが json も受けるし、doctor の 2 値制約は
`Option<String>` のまま runtime にしか無いからだ。

## 優先度
- 順位: 8 / 全候補数 9
- RICEスコア: 2.7（R=2 / I=1 / C=2 / Effort=1.5日）
- 根拠: 利用者の見え方は help 1 行の正確さで、振る舞いは変わらない。I=1、C=2 は
  「文面の修正は確実だが、doctor の型を狭める方法に判断が残る」ため。PBI 45 で
  `FormatArgs` group を文面修正と同一 PR の条件付きとして先送りした残り。

## ビジネス価値
- help が実態と一致し、`--format json` を試した利用者が「表示に無い値」で迷わない。
- `--format` の宣言が 1 箇所になり、9 view に散った重複が消える。
- 測定方法: 7 view の help が `table | markdown | json` を表示すること。doctor の
  拒否 message が不変であること。golden の差分が `--format` の行だけに閉じること。

## 背景（2026-11-02、PBI 45 の先送り分）
- `src/cli/mod.rs` の 7 view（usage `:92`、inventory `:119`、waste `:135`、overhead
  `:144`、prompts `:156`、failures `:174`、stuck `:183`）が
  「Output format: table | markdown.」と書く。しかし `report.rs` の `parse_format`
  （`:34-40`）は json も受け、`docs/specs/cli.md:178` は「Machine (`--format json`,
  on every command)」と明記している。trend（`:104`）だけが 3 値を書いている。
- doctor（`:200`）は「table (human text) | json (the routed findings)」。実体は
  `ViewFormat::TableJson`（`src/cli/report.rs:49-73`）で、`src/cli/window.rs:179` が
  それを通す。ただし clap のフィールドは `Option<String>`（`src/cli/mod.rs:202`）で、
  2 値制約は runtime の parse にしかない。
- PBI 45 は `FormatArgs` group を「文面修正と同一 PR の条件付き」として作らなかった
  （台帳 238-240）。文面だけ直すと宣言は 9 箇所に散ったままなので、本 PBI で併せて
  片付ける。
- help の bytes は `tests/cli_help_golden.rs:16-27` の `PINNED`（9 行）で pin されて
  いる。文面を変えると必ず赤くなるので、golden の更新を同一 PR に含める必要がある。
- 根本原因: 旗の語彙（`ViewFormat`）は 1 箇所に寄ったが、clap の宣言と help 文面は
  view ごとに手書きのまま残った。

## BDD受け入れシナリオ

```gherkin
Scenario: 7 view の help が受け付ける値を全部書く
  Given saniwalens usage --help
  When help を読む
  Then --format <FORMAT> の説明が table | markdown | json になる
  And 他の行の文言と順序は変わらない

Scenario: doctor は 2 値のまま
  Given saniwalens doctor --help
  Then --format の説明は table (human text) | json のまま
  And doctor --format markdown の拒否 message は不変

Scenario: 宣言の重複が消える
  Given FormatArgs group を windowed view に flatten した
  Then --format の宣言は shared 1 箇所 + doctor 1 箇所だけになる
  And 9 view の help の bytes が pin どおり
```

## 受け入れ基準
- [ ] `src/cli/window.rs` に `FormatArgs { #[arg(long)] format: Option<String> }` を
  追加し、3 値の文面を 1 箇所に持つ。trend を含む windowed view と `sql` の重複宣言を
  置き換える。doctor は 2 値の文面を自分の宣言として残す（group に入れない）
- [ ] 7 view の文面を `Output format: table | markdown | json.` に直す
- [ ] doctor の引数型を 2 値へ狭める。ただし clap の `value_parser` を入れると拒否
  message が clap の文面に変わり、`tests/windowed_flag_vocabulary.rs:188-225` の
  exact pin（`unknown --format 'markdown' (table|json)`）が壊れる。推奨: runtime の
  `ViewFormat::TableJson::parse` を入口に残したまま、doctor 以降に渡す型を解決済みの
  `Format` に狭める（`open_scoped_table_json` の中で parse を終える）。clap の
  value_parser を選ぶなら拒否 message の pin と `docs/specs/cli.md:202-208` を同一 PR
  で書き換える
- [ ] `tests/cli_help_golden.rs` の 7 定数を新しい bytes に更新する。`PINNED` は 9 の
  まま。差分が `--format` の行（と桁揃えの連動）だけであることを PR 本文に記録する
- [ ] flatten の位置を現在の `--format` の位置（`period` の直後、`read` の前）に合わせ
  る。位置が動くと help の行順が変わり、golden の差分が意図より広がる
- [ ] `tests/windowed_flag_vocabulary.rs:159-186`（7 view が markdown を受ける）と
  `:188-225`（doctor の拒否）が green のまま
- [ ] `docs/specs/cli.md` を同期する。`:98` の usage synopsis、`:202-208` の doctor の
  語彙。`:178` の「every command」と 7 view の文面が一致することを確認
- [ ] `sql` の文面をどうするかを決める。推奨: 同じ 1 行修正を同 PR で行う（`sql` は
  `parse_format` を通り json を受ける: `src/cli/mod.rs:382-386`）。除外するなら理由を
  PR 本文に 1 行
- [ ] fmt、clippy、フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `tests/cli_help_golden.rs` の byte 比較（7 view 更新、trend / doctor は不変）
- `every_windowed_views_help_is_pinned_byte_for_byte`（`:318`）と view 集合の一致
  （`:248`）が green のまま

### 統合テスト
- `tests/windowed_flag_vocabulary.rs:159-186`: 7 view が実際に markdown で走る
  （文面と実装の一致の根拠）
- `:188-225`: doctor の拒否 message と、store open 前の拒否

### 単体テスト
- `parse_format`: `table` / `markdown` / `md` / `json` / 不正値の message
- `ViewFormat::TableJson::parse`: `table` / `json` / 不正値の message（doctor の 2 値）
- `FormatArgs` を flatten した 1 view の clap 定義から、`--format` が 1 つだけ取れる
  こと（重複宣言の再発検知）

## 実装アプローチ
- **Outside-In**: 7 view の help golden を先に新しい bytes へ（赤）-> `FormatArgs`
  group -> doctor の型 -> spec 同期 -> 検証。実装は subagent-driven-development で
  行う。

## 見積もり
1.5 SP（E=1.5日。RICE 2.7）

## 技術的考慮事項
- 依存関係: PBI 45 の着地後（`WindowedRead` / `ViewFormat` の分割が前提）。`src/cli/mod.rs`
  を PBI 57 / 58 / 59 と共有するので番号順に直列（57 -> 58 -> 59 -> 60）
- `FormatArgs` は `#[command(flatten)]`。help 上の位置は flatten した場所になるので、
  現在の位置に置く
- doctor の 2 値は「語彙」の問題で、`ViewFormat` がすでに持っている。型を狭めると
  きに新しい enum を増やさない（`Format` に解決してから渡す）
- 非機能要件: なし

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/cli/mod.rs` の `--format` 宣言と doc: usage `:92-94`、trend `:104-106`、
  inventory `:119-121`、waste `:135-137`、overhead `:144-146`、prompts `:156-158`、
  failures `:174-176`、stuck `:183-185`、doctor `:200-202`、sql `:212-214`。
- `src/cli/report.rs:28-39` `Format`、`:34-40` `parse_format`、`:49-73` `ViewFormat`
  と `parse`。
- `src/cli/window.rs:179` の `ViewFormat::TableJson` 経由の open。
- `tests/cli_help_golden.rs:16-27` `PINNED`、`:29-192` の 9 定数、`:318` の byte 比較。
- `tests/windowed_flag_vocabulary.rs:159-186`、`:188-225`。
- `docs/specs/cli.md:98`（usage synopsis）、`:178`（machine format は全コマンド）、
  `:202-208`（doctor の語彙）。
- 既実装の確認: `FormatArgs` は存在しない（grep で 0）。7 view の文面は
  「table | markdown」のまま。よって本 PBI は文面修正 + 宣言の集約。

### 実装手順
1. 基準の capture: 9 view の `--help` を取り、差分の比較元を残す
2. `FormatArgs` を `src/cli/window.rs` に追加し、7 view + trend + sql の宣言を置換
3. 7 view の文面を 3 値に（doctor はそのまま）
4. golden の 7 定数を再 capture し、差分を確認
5. doctor の型を狭める（runtime parse を入口に残す）
6. spec 2 箇所と synopsis を同期
7. 検証: `cargo test --test cli_help_golden`、`--test windowed_flag_vocabulary`、
   フルスイート、fmt、clippy

### 落とし穴
- `--format <FORMAT>` の幅が変わると、同じ Options ブロックの他の行の桁揃えが動く。
  golden の差分は当該行だけとは限らない。目視で確認し、意図した差分だけを PR 本文に
  列挙する
- `md` は parse で markdown に解決されるが help には出さない（現行も出していない）。
  文面に足すと pin が動くので足さない
- doctor の拒否 message は PBI 45 が exact pin している。clap の value_parser に
  変えると message が `invalid value 'markdown' for '--format <FORMAT>'` 系になり、
  spec の引用も変わる。両方同時に直すか、runtime parse を残す
- `FormatArgs` を doctor にも flatten すると 2 値の文面が消える。doctor は別宣言
- 文面だけ直して group を作らないと、次に view を足したときに同じ重複が再発する。
  本 PBI の受入は「1 箇所 + doctor」まで含む
- `tests/windowed_flag_vocabulary.rs:28-42` の view 導出は `--last` の有無で判定して
  いる。`FormatArgs` を足しても `--last` の位置は変わらないので影響しない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 7 view の help が 3 値を表示する
- [ ] `--format` の宣言が shared 1 箇所 + doctor 1 箇所
- [ ] doctor の拒否 message が不変（または spec と pin を同時更新）
- [ ] golden の差分が意図した行だけ（PR 本文に記録）
- [ ] cli.md の 3 箇所が同期している
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
