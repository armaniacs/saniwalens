# PBI: trend の n/a の理由文を実際の理由と一致させる（point kinds の適用外行の解消）

## ユーザーストーリー
分析者として、trend の各行の n/a に実際の理由が一致した表示がほしい、
なぜなら理由が偽のままだと「比較不能」と「比較できない種類の指標」の区別が
つかず、 Reporting honesty の「近似は隠さずラベルする」を満たさないからだ。

## ビジネス価値
- 比較不能（前側が空）と metric 適用外（point kinds の seconds / out tokens）
  が同じ n/a 1 語に畳まれず、行として読める情報だけが残る
- 測定方法: point kinds のみのストアで trend が「seconds」「out tokens」行を
  出さないこと。前側に実測があるのに丸めで 0.0 に見える場合も n/a に潰れず
  差分が正しい向きで出ること（2026-10-31 のなぜなぜ分析で実証済みの現象）

## 背景（2026-10-31 のレビュー持ち越し指摘3、なぜなぜ分析の実証）
- point kinds（session_start / prompt / agent_spawn / tool_use / tool_error /
  bash_cmd / file_edit）は ingest が `duration_sec = 0`、`out_tokens = 0` を
  定数で書く（src/store.rs:785, 836, 870, 977, 1015 付近、
  docs/specs/events.md:77-80「span は現在 skill_invocation のみ」）ため、
  これらの kind の trend「seconds」「out tokens」行は常に
  `0.0 | 0.0 | n/a | n/a` になる
- preamble「A delta marked n/a: the previous side holds no events」
  （src/cli.rs:1998-2000）が全 n/a 行に一律で付き、point kinds の行では
  **偽の理由**を報告する
- 根本原因: 「空」を `T::default()` センチネル（値の領域）で表現し、
  `kind_pairs` の `Option<KindRollup>`（src/cli.rs:1790-1805）を
  `map_or(0.0)` で捨てたため、「空 / 測定ゼロ / 丸めゼロ / 不適用」の
  4 意味が 0.0 → 1 状態 → n/a 1 語に潰れた
- `delta_cells_float` は判定前に `display_precise` で丸める
  （src/cli.rs:1891）ため、実測 0.04s が丸めで 0.0 と等しくなり n/a に
  潰れる（テスト未 pin）
- `docs/specs/cli.md` 134 行目付近自体が「emptiness makes n/a」と偽の因果を
  spec に書いた（実装から spec への逆流）

## BDD受け入れシナリオ

```gherkin
Scenario: 適用される metric だけが行に出る
  Given ストアに point kind（file_edit 等）のイベントしかない
  When ユーザーが `saniwalens trend` を実行する
  Then 「seconds」と「out tokens」の行は出ない
  And 適用される「events」の行だけが各 kind に出る

Scenario: 理由文が実際の理由と一致する
  Given 前ウィンドウにイベントが無いストアがある
  When ユーザーが trend を実行する
  Then 「前側が空のため比較不能」の注記は実際に前側が空のときだけ出る
  And 適用外の指標の行には比較不能の注記が付かない（行自体が無い）

Scenario: 測定ゼロと空は区別される
  Given 前ウィンドウに duration 0.04s のイベントがある
  When ユーザーが trend を実行する
  Then その比較は n/a にならず、表示精度での差分と向きが出る
  And 空の前側だけが n/a として出る
```

## 受け入れ基準
- [x] kind に適用されない metric の行を省略する（usage ビューの「空セクション
  省略」慣習の行単位拡張、src/cli.rs:2026-2028 付近）。適用の判定は
  `duration_sec` / `out_tokens` がその kind で意味を持つかで行い、
  point kinds（session_start / prompt / agent_spawn / tool_use / tool_error /
  bash_cmd / file_edit）は events 行のみにする
- [x] 「空」を値でなく型で運ぶ: `kind_pairs` の `Option<KindRollup>` を
  判定まで保持し、`map_or(0.0)` のセンチネル畳み込みを撤去する
  （src/cli.rs:1790-1805、:1904-1943）。測定ゼロ（丸めで 0.0 に見える実測）と
  空（行不在）を区別する
- [x] preamble / note の理由文を理由別に修正する: 前側が空のときだけ既存の
  「holds no events」文を出し、適用外の行は出ない。src/cli.rs:1998-2000 と
  :2112-2118 付近の文言を確認する
- [x] spec 更新: `docs/specs/cli.md` Views 表の trend 行に適用 metric の注記
  （point kinds は events のみ）を追記し、134 行目付近の「emptiness makes
  n/a」の偽の因果を修正する。Reporting honesty に「n/a の理由文は実際の理由
  と一致する」条項を追加する
- [x] テスト pin: point kind の「seconds」行が出ないこと、preamble 文言が
  前側空のときだけ出ること、丸め境界（実測 0.04s → 表示 0.0 でも n/a に
  ならないこと）
- [x] 既存 8 ビューと `sql` / `analyze` / `optimize` の出力は一切変わらない
- [x] `just check` と `cargo test` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- point kinds のみの合成ストアで trend markdown に「seconds」行が無い

### 統合テスト
- 前側が空のストアで preamble 文言が出る、前側に実測があるストアで差分が出る

### 単体テスト
- `trend_kind_rows` の適用判定（point kinds は events 行のみ）
- `delta_cells_float` の丸め境界（0.04s は n/a にならない、空は n/a）

## 実装アプローチ
- **Outside-In**: E2E（point kinds の markdown）から失敗を確認し、
  適用判定（単体）→ Option での空運び（単体）→ 理由文（統合）の順に
  Red-Green-Refactor
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0: R=2 / I=2 / C=3 / E=1日）

## 技術的考慮事項
- 依存関係: PBI 29（--last 暦外）と独立、どちらが先でも良い。cli.rs の同じ
  領域（trend 表示部）に触れるため**直列で実施する**
- テスタビリティ: `trend_kind_rows` / `delta_cells_float` は純粋関数、
  合成 fixture で e2e 可能（tests/trend_markdown.rs の型）
- 非機能要件: プライバシー（合成 fixture のみ）、既存 8 ビュー出力のバイト同一

## 実装者向け注記

### 現状コードの確認（2026-10-31 実施済み）
- `src/cli.rs` の `direction_of`（1744-1753行目付近）: `previous ==
  T::default()` を Unavailable にするセンチネル判定。変更しない（後段の
  Option 運びで空だけが Unavailable に流れるようにする）
- `kind_pairs`（1790-1805行目付近）: `BTreeMap` によるペアリング、両側
  `Option<KindRollup>`。ここが型での空運びの起点
- `trend_kind_rows`（1904-1943行目付近）: 全 kind × 3 metric の固定行。
  適用判定を入れる場所
- `two_window_store` でも seconds 行が常に n/a だが、テストは events 行のみ
  assert（src/cli.rs:4375-4379 付近）。テストを広げる

### 実装手順
1. 適用判定の純粋関数（kind が duration / out_tokens を持つか）を追加する
2. `trend_kind_rows` で適用されない行を省略する（空セクション慣習の拡張）
3. `kind_pairs` の Option を `trend_kind_rows` / `trend_json` まで保持し、
   空（None）だけが Unavailable に流れる形に変える
4. preamble / note の理由文を理由別に修正する
5. テスト pin（point kind の省略、丸め境界、文言）を追加する
6. `docs/specs/cli.md` と Reporting honesty を更新する

### 落とし穴
- `direction_of` を変えると skills / totals の表示も変わる。変更は
  `kind_pairs` 以降（Option の保持）に留め、`direction_of` 自体は触らない
- 「seconds」行の省略で表の見出し行数が変わる。空セクション省略の慣習
  （見出しだけの表を出さない）と揃える
- JSON の kinds から seconds / out_tokens を消すか残すか: JSON は機械契約
  なので**残し、delta を null にする**方が契約破壊が小さい。表だけ省略する
  判断も可能だが、どちらかを PBI の判断として明記してから実装する

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] 既存 8 ビュー等の出力が capture / 回帰で不変と確認
- [x] `just check` と `cargo test` が green
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/cli.md 更新済み（Views 表 + Reporting honesty）
