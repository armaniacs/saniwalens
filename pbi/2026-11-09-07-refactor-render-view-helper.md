# PBI C4: render_view 共通化 + 5ビュー移行

## ユーザーストーリー
saniwalens の保守担当者として、view 描画の儀式（eprint_window → JSON → empty → preamble(framed) → render）を単一ヘルパー `render_view` にまとめたい。なぜなら 5 ビュー × 9 arms の重複が表記ゆれ（bespoke empty、inline window 判定、手書き projection）の温床であり、mod.rs 分割（C6）の前提になるからだ。

## 優先度
- 順位: 7 / 全11件
- RICEスコア: 2.0（Reach=10 / Impact=1 / Confidence=100% / Effort=5）
- 依存: C6 の先行（描画ヘルパー先行、mod.rs 分割は C4 の後）

## 背景
- 儀式の重複: `if Json { emit_json(with_window) }` が `src/cli/views/models.rs:27-42`、`prompts.rs:29-63`、`sessions.rs:68-86`、`trend.rs:673-`、`waste.rs:205-` に分散。
- 9 arms が同一儀式を呼ぶ: `src/cli/mod.rs:917,965,1434,1595,1719,1817,1931,2049,2131`。
- 反復の実例: `eprint_window → JSON → empty → preamble(framed) → render` が `sessions.rs:65,83,91,98-106,120-134` と `models.rs:23,28,47,54-62` で重複。
- ドリフト: `prompts.rs:70-84` の bespoke empty、`mod.rs:2150` の inline `window.is_all`（`window.rs:380` の `unused_label` と二重管理）。
- 根因: `report.rs:81-83` で Json が到達不能のため各 view が手書き projection を強いられる。

## BDD受け入れシナリオ
Scenario: table 表示は移行前後で同一
  Given 合成フィクスチャの store が用意されている
  When  5 ビュー（models/prompts/sessions/trend/waste）を table 形式で実行する
  Then  help・table 出力が移行前とバイト同一である

Scenario: JSON 表示は移行前後で同一
  Given 同一フィクスチャの store が用意されている
  When  5 ビューを `--format json`（window あり/なし）で実行する
  Then  JSON 出力が移行前とバイト同一である

Scenario: 空結果と window 注記は共通ヘルパー経由になる
  Given 結果が空の view と window 指定ありの view がある
  When  各 view を実行する
  Then  empty 文言と window 注記が `render_view`（+ window.rs 語彙）経由で出力される
  And   prompts bespoke empty と mod.rs inline 判定が残っていない

## 受け入れ基準
- [ ] `render_view`（eprint_window → JSON → empty → preamble → render を一括）が新設され 5 ビューが移行済み
- [ ] help / table / JSON 出力が移行前とバイト同一（golden で固定）
- [ ] `prompts.rs:70-84` bespoke empty が削除され共通 empty 経路に一本化
- [ ] `mod.rs:2150` inline `window.is_all` が削除され `window.rs:380` 語彙に一本化
- [ ] `cargo test`・`cargo clippy --all-targets --all-features -- -D warnings` が green
- [ ] C6（mod.rs 分割）が着手可能な状態（deliver + 描画が views 側に寄っている）

## テスト戦略
- Golden: 5 ビュー ×（table / json / json+window / 空）の golden 出力を移行前後でバイト比較
- CLI help ピン: `--help` 出力のスナップショットで help 文言の不変を固定
- 単体: ヘルパーの分岐（Json / 空 / window あり・なし・all）を合成 fixture でカバー
- TDD: Outside-In（受け入れ＝golden red → green → 重複削除のリファクタ）

## 見積もり
5（RICE の Effort と同一根拠。5 ビュー移行 + golden 整備を含む）

## DoD
- 受け入れ基準の全項目が満たされること
- 振る舞い保存（help/table/JSON バイト同一）が golden/cli_help で裏取り済み
- 既存テスト全 green、clippy `-D warnings` green
- C6 への申し送り（残る deliver/描画の切り分け点）が本ファイル末尾または C6 PBI に記録される

## 実装者向け注記
- 振る舞い保存のみ。文言・順序・JSON キーの変更は禁止（変更が必要なら別 PBI 化）
- `report.rs:81-83` の Json 到達不能は本 PBI では直さない（手書き projection は温存）。直すなら C6/C7 側で扱う
- window 語彙は `window.rs:380` に寄せ、`unused_label` の扱いを壊さない

## 検証記録 (B5統合・2026-11-09)

- 実装: `ViewCtx`＋`TableSpec`＋`render_view` 単一強制点、5ビュー＋mod.rs 6view arm移行。prompts bespoke分岐→`prompts_empty_message`、inventory `is_all`→`unused_status` 一本化。
- 既存バグ発見→統合側で修正: `Store::session_breakdown` のbind順逆転（窓指定で常に0件）。`efilter` がSQL先出のためepoch bindsを先に。回帰テスト `session_breakdown_window_keeps_matching_sessions` 追加（Red→Green確認済み）。golden `sessions_json_window.txt` はバグ出力（`[]`）でblessされていたため、修正後の正出力でre-bless（内容目視済み：窓内2セッションのtokens/friction付き）。
- 境界検証: fmt OK、clippy 0 errors、lib 645 passed、golden 28/28。
