# PBI: trend payload に baseline の不在・実測ゼロ・適用外を区別させる

## ユーザーストーリー
利用者として、前の window にイベントがあるのに出力トークンが 0 だった場合、
trend が「比較不能」ではなく実際の差分（例: +300 / up）を見せてほしい、なぜなら
現行の typed payload は 0 を「基準なし」と同じ状態に潰し、比較を失っているからだ。
機械可読な JSON の消費者としても、n/a の理由（基準なし / 適用外）を区別したい。

## 優先度
- 順位: 4 / 全候補数 10
- RICEスコア: 8.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.5日）
- 根拠: PBI 39 が table のバイトを変えて比較を失った回帰の修正。trend.rs 1 ファイルに
  閉じるが、JSON 契約に `reason` を足すため golden と spec の同時更新が必須。
  ⑪（scratch guard）の前提として先行する。

## ビジネス価値
- 前 window に基準が実在する metric は、値が 0 でも比較される（n/a に潰れない）。
- n/a の理由が「基準なし」と「適用外」で区別され、table の理由文・note・JSON が
  同じ payload 判断を読む。
- 測定方法: present-zero の合成 store で `out tokens` が `+300 | up` を出し、
  n/a 行が無いので理由文も出ないこと。point kind は table で行を落とし、
  JSON は `reason: "not_applicable"` を返すこと。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 10ケース）
- `src/cli/trend.rs:212-220` の `map_or` が `Option<KindRollup>` を捨て、
  不在・実測ゼロ・適用外を 1 状態に潰す。判断は値センチネル（:56-61, :171-174）、
  理由文は events のみ（:479-482）、note は `agg.previous.by_kind` 直読み（:535）、
  JSON は別経路で推測している。
- PBI 39（d8325e4）は table のバイトを変えた。present-zero の合法 store で
  `out tokens: +300 | up` が `n/a | n/a` になり、理由文も note も出ない。
  旧 table は Option の存在で判定し、旧 JSON は値で畳んでいたため、当時から
  table/JSON は不一致だった。PBI 39 の「出力 byte-identity」受入はこの入力クラスで偽。
- 根本原因: payload が「previous に基準が存在したか」を値（0/0.0）でしか表現できない。

## BDD受け入れシナリオ

```gherkin
Scenario: previous に実測ゼロの基準がある metric は比較される
  Given previous window に out_tokens=0 の skill_invocation がある
  And current window に out_tokens=300 の skill_invocation がある
  When `trend --format table` を実行する
  Then out tokens 行は "n/a" ではなく "+300" と "up" を出し、n/a の理由文は出ない

Scenario: 基準が無い metric は理由付きの n/a になる
  Given previous window にイベントが無い
  When `trend --format table` を実行する
  Then 該当行は n/a になり、previous window が空である理由文と note が出る

Scenario: 適用外の metric は table で省略され JSON で理由を持つ
  Given file_edit のような point kind だけがある
  When `trend --format table` と `--format json` を実行する
  Then table は out tokens / seconds 行を出さず、
       JSON は各 metric に "reason": "not_applicable" と null delta を持つ
```

## 受け入れ基準
- [ ] `Baseline<T> { Absent, Measured(T) }` と
  `UnavailableReason { NoBaseline, NotApplicable }` を trend.rs に追加する
- [ ] `Metric<T>` に `reason: Option<UnavailableReason>` を持たせ、
  `direction == Unavailable` と `reason.is_some()` の同値を不変条件にする
- [ ] metric 構築を 1 関数に集約し、`Baseline::Measured(0)` / `Measured(0.0)` は
  表示精度 0.1 で比較する。`Absent` だけが `NoBaseline` になる
- [ ] point kind の適用外は `NotApplicable` + `delta: None` とし、table の行省略は
  payload の reason を読んで決める（PBI 30 の決定を維持）
- [ ] `TrendPayload` に `previous_window_is_empty()` と `has_no_baseline()` を追加し、
  note と preamble の理由文を payload 経由にする。`trend()` は `agg` を直読みしない
- [ ] JSON の各 metric に `"reason": null | "no_baseline" | "not_applicable"` を追加し、
  `tests/trend_json_golden.rs` を再 pin する。present-zero の第 2 ケースを追加する
- [ ] table の present-zero 回帰テストを追加する（`tests/trend_markdown.rs`）
- [ ] 理由文を「その行の previous side に比較対象が無い」と行スコープで明示し、
  kind / skill / totals の 3 ケースを 1 文で覆う
- [ ] spec 更新: cli.md の trend 行、machine contract の `reason`、Reporting honesty の
  n/a 条項。PBI 39 の byte-identity 基準がこの入力クラスで反転したことを台帳に記録する
- [ ] `store.rs` / `report.rs` / `mod.rs` は触らない。fmt/clippy/フルスイート green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- present-zero の table 出力（`+300 | up`、理由文なし）
- previous が空の table 出力（n/a + 理由文 + note）
- point kind の table 行省略と JSON の `not_applicable`

### 統合テスト
- `tests/trend_json_golden.rs` の golden を `reason` 込みで再 pin
- markdown の既存 4 テストが不変で green

### 単体テスト
- `Metric` の不変条件（Unavailable と reason の同値、NotApplicable と delta None）
- `Measured(0)` / `Measured(0.0)` の比較
- `Absent` が events / skills / totals の 3 経路で `NoBaseline` になる
- 「描画された n/a 行 > 0 なら理由文か note がある」不変条件を 5 形状で回す

## 実装アプローチ
- **Outside-In**: present-zero の table 回帰を赤にし、次に payload の型を入れ、
  最後に golden と spec を更新する。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
1.5 SP（E=1.5日。RICE 8.0）

## 技術的考慮事項
- 依存関係: なし。⑪（scratch guard）が `tests/trend_json_golden.rs` を触るため先行する
- JSON 契約の変更（`reason` 追加）は消費者の同意が必要。spec に明記する（CHANGELOG は不変）
- count 系の実測ゼロは、イベント行が存在しない限り従来どおり `NoBaseline` とし、
  意図的な据え置きを spec に書く
- 非機能要件: プライバシー（合成 store のみ）、table の byte 変更は present-zero に限定

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `src/cli/trend.rs`: `direction_of` :56-61、`Metric` / `KindPayload` :68-86、
  `trend_payload` :200-248、理由文 :479-482、note :535-541、単体 :736-792。
- `tests/trend_json_golden.rs`: golden :136-248、fixture に present-zero が無い。
- `tests/trend_markdown.rs`: n/a 理由の 4 テスト :118-196。
- `docs/specs/cli.md:138`（trend 行）、:163-171（machine contract）、
  :338-345（Reporting honesty）。
- PBI 39 archived: `pbi/archived/2026-11-01-39-refactor-view-payload-shaping.md`。

### 実装手順
1. Red: present-zero の table 回帰テストと `Metric` 不変条件テストを書く
2. `Baseline` / `UnavailableReason` / `Metric.reason` を追加し、`map_or` を撤去
3. payload のメソッドで note と理由文を駆動し、`agg` 直読みを消す
4. table のセル・行を reason ベースにし、point kind の省略を維持
5. golden を `reason` 込みで再 pin、present-zero ケースを追加
6. spec を同一コミットで更新（CHANGELOG は不変）

### 落とし穴
- `direction == Unavailable` の判定を値センチネルに戻さない。reason と同値にする
- point kind の適用外で理由文を出さない（PBI 30 の偽理由が戻る）
- n/a 行が 0 のときに理由文や note を出さない（現行テストが pin）
- JSON に散文を入れない。`reason` は enum の機械可読な値だけ
- `Measured(0.0)` の float 比較は表示精度 0.1 を維持する

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] present-zero の table が比較を出し、n/a に潰れない
- [ ] JSON が `reason` を返し、golden が再 pin されている
- [ ] note と理由文が payload 経由で、`agg` 直読みが無い
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] cli.md 更新済み（CHANGELOG は不変）
