# PBI: 暦 bounds の恒久 sweep を oracle 強化と代表 zone に広げる

## ユーザーストーリー
保守担当者として、period の bounds が「暦が名指せる日付を描く」という不変条件を
テストで実際に検査してほしい、なぜなら現行 sweep は非空と offset 接尾辞しか見ず、
暦外の日付を描いても緑のまま通り、chrono アップグレード時の退行を検知できないからだ。

## 優先度
- 順位: 2 / 全候補数 10
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.0日）
- 根拠: 契約の正しさ（暦の不変条件）を守る恒久検知。zone 拡大単独では検知が増えない
  ことが probe で確定したため、oracle 強化を本体にする。

## ビジネス価値
- 「accepted window の bound は暦の名指せる日付を描く」がテストで表現され、
  chrono の暦レンジ・書式・丸めの変更が byte 差分として落ちる。
- 測定方法: CAL_MAX x JST の暦外描画を byte pin し、暦内は RFC 3339 の
  round-trip で同一 instant に戻ること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 6本）
- `src/core/period.rs:283-300` の `bounds()` は
  `timestamp_opt(...).expect("an in-calendar instant renders in any valid offset")` を持つ。
- 恒久 sweep `every_accepted_window_renders_non_empty_bounds`（:800-904）は
  「非空 + offset 接尾辞」だけを検査する。CAL_MAX x JST は
  `+262143-01-01T08:59:59+09:00` を描くが、`NaiveDate::MAX` は `+262142-12-31` で、
  この日付は暦に存在しない。それでも sweep は緑。
- 全 valid offset 172,798 点で `timestamp_opt` の None は 0 件。offset 依存の穴は
  今日存在せず、zone を増やしても検知は増えない。暦外になるかは offset の符号で決まり、
  JST と -8h が既に両符号を覆っている。
- 根本原因: sweep の oracle が、守ると称する不変条件より厳密に弱い。
  zone 拡大は症状への対処であり、原因（検査が不変条件を表現していない）への対処ではない。

## BDD受け入れシナリオ

```gherkin
Scenario: 暦内の bound は同じ instant に戻る
  Given resolve が受け付けた window がある
  When bounds を描画して RFC 3339 で parse する
  Then parse 結果の timestamp は元の epoch と一致する

Scenario: 暦端の presentation 溢出は byte pin される
  Given clock が CAL_MAX で zone が JST である
  When bounds を描画する
  Then 描画は "+262143-01-01T08:59:59+09:00" に一致し、
       これが仕様上の限界であることがコメントと spec に書かれている

Scenario: 代表 zone と分単位 offset で panic しない
  Given 10 個の代表 offset と全 2,879 の分単位 offset がある
  When 暦端の epoch を bounds に通す
  Then panic せず、描画は非空で、暦内なら round-trip する
```

## 受け入れ基準
- [ ] sweep の `check` を「非空 + 接尾辞」から「暦内なら RFC 3339 parse で
  instant 一致、暦外なら byte pin」に強化する
- [ ] date 経路の zone を 10 個の代表値に広げる:
  `[JST, -28800, 0, 60, -60, 5400, -5400, 32400, -32400, 86340, -86340]`
  （JST を含めて 11 点。分単位の端と時間外 offset を覆う）
- [ ] suffix 式を時間単位前提から `{:02}:{:02}` の分対応に一般化する
- [ ] 全 2,879 の分単位 offset x 4 端点で `bounds` を直接呼ぶテストを追加する
  （panic しない、非空、暦内なら round-trip。+25-40ms 程度）
- [ ] CAL_MAX x JST と CAL_MIN x -8h の 2 例を byte pin する
- [ ] `expect` の文言を「TimeRange holds an in-calendar epoch: resolve validates
  both ends」に変え、原因の所在を呼び出し側だと分かるようにする
- [ ] spec 3 点を同一変更で更新する: 暦端の presentation 溢出は最大 1 zone 幅で
  実クロック到達不能、`--tz` の domain は分単位（-86,340..=86,340 の 60 秒刻み）、
  RFC 3339 の年範囲は 0..=9999 に限る注記
- [ ] 非テストコードの変更は expect 文言 1 行だけ。`bounds` の `Result` 化はしない
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存の日付・span エラー E2E（`tests/period_errors.rs`）が不変で green

### 統合テスト
- `every_accepted_window_renders_non_empty_bounds` の強化
- 全分単位 offset の `bounds` 直呼びテスト

### 単体テスト
- 暦内 round-trip、暦外 byte pin 2 例、代表 zone の suffix
- CAL_MAX x JST が旧 oracle で緑・新 oracle で赤になること（変異確認）

## 実装アプローチ
- **Outside-In**: 旧 oracle が暦外描画を通過することを再現 -> oracle 強化で赤 ->
  byte pin で green -> zone/分対応 -> 全数 sweep -> spec。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
1 SP（E=1.0日。RICE 12.0）

## 技術的考慮事項
- 依存関係: なし。`src/core/period.rs` と `docs/specs/cli.md` のみ
- テスタビリティ: `bounds` は pub なので直接呼べる
- 非機能要件: `proptest` などの dev-dependency は追加しない（分単位全数で domain を覆う）
- `+24:00` は RFC 3339 の offset として不正。`%:z` が分に丸める事実を仕様に書く

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `src/core/period.rs`: `bounds` :283-300、sweep :800-904、suffix 式 :836-840、
  clocks/spans/dates :815-833。
- `docs/specs/cli.md`: Period 節 :186-243、Reporting honesty :276-308、
  `--tz` の記述 :263。
- 旧 oracle の弱点: CAL_MAX x JST を描いても `ends_with("+09:00")` だけで通る。

### 実装手順
1. Red: `check` に round-trip を足し、CAL_MAX x JST が落ちることを確認
2. 暦外 2 例を byte pin して green
3. zone を代表 11 点へ、suffix を分対応へ
4. 全 2,879 offset x 4 端点の直呼びテストを追加
5. expect 文言とコメントを更新
6. spec 3 点を同一コミットで更新

### 落とし穴
- 秒単位 172,799 offset は `%:z` の分解能が分なので検知を増やさず 48 秒かかる。采用しない
- `timestamp_opt` は UTC 側の暦だけを見る。offset 依存の None を仮定しない
- 暦外を refuse する `Result` 化は呼び出し 5 箇所 + 新 error family で過剰。実クロック到達不能
- 既存の `stop` 系テストや PBI 37 の判断を撤回しない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 旧 oracle では通る暦外描画が新 oracle で落ちることを確認
- [ ] 代表 zone と全分単位 offset の sweep が green
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] cli.md の 3 点を更新済み
