# PBI: headless renderer 選択を harness seam に移す（run_headless の最小 seam test 付き）― B slice

## ユーザーストーリー
実装者として、headless run の run-line renderer 選択が harness seam の背後に
あってほしい、なぜなら shell-local な選択（opencode の render_run_line を直接
消費）が format isolation の契約に沿わず、run_headless の heartbeat/thread
logic がどの seam でも untested だからだ。

## 優先度
- 順位: 42 / 全候補数 7
- RICEスコア: 6.0（Reach=3 / Impact=2 / Confidence=2 / Effort=2日）
- 根拠: PBI 38（A slice）に依存するため最後。kilo は OpenCode fork なので
  headless kilocode も seam の内側で自然に成立する

## ビジネス価値
- headless run の renderer 選択が harness seam の per-harness fact になり、
  kilo headless が追加 harness 編集なしで成立する
- 測定方法: run_headless が seam が選んだ renderer を受けること。
  run_headless の最小 seam test が着地すること

## 背景（なぜなぜ分析 2026-11-01 round3、A slice の続き、file:line 実証）
- headless renderer 選択（opencode_mod::render_run_line :1002 の直接消費）は
  shell-local。run_headless（:926-1026）の spawn/log/heartbeat（:947-987）は
  tests/ で 0 件（probe 実測）― どの seam でも untested
- PBI 38（A slice）が launch shape（binary/argv/RunMode）を data arms へ移設
  する前提で、本 PBI は renderer 選択の移設と run_headless の最小 seam test
- 根本原因: Harness seam の集約が analyze 側に限られ、launch 形状と renderer
  選択が shell に住む（A slice で launch 側を閉じ、本 PBI で renderer 側を
  閉じる）

## BDD受け入れシナリオ

```gherkin
Scenario: renderer 選択は harness seam の fact である
  Given adapter/harness.rs に headless run-line renderer の per-harness 事実がある
  When run_headless が headless run を行う
  Then renderer は seam が選んだもので、shell-local な match は存在しない

Scenario: run_headless の heartbeat が 1 箇所で testable になる
  Given run_headless の最小 seam test がある
  When test を実行する
  Then heartbeat/thread logic が 1 箇所で pin される（tests/ 0 件から 1 件へ）

Scenario: kilo headless が追加編集なしで成立する
  Given kilo は OpenCode fork である
  When headless kilocode を考える
  Then seam の内側で自然に成立する（shell 編集不要）
```

## 受け入れ基準
- [ ] headless run-line renderer の選択を adapter/harness.rs の data arms へ
  移設する（PBI 38 の launch shape 移設に続く、input_unit/copy/db_file_name/
  launch shape の隣）
- [ ] run_headless の spawn/log/heartbeat は harness 非依存のまま、seam が
  選んだ renderer を受ける
- [ ] run_headless の最小 seam test を追加する
  （heartbeat/thread logic を 1 箇所で testable に。tests/ で 0 件 → 1 件）
- [ ] kilo headless gate の code comment（:639-647）の判断を維持する
- [ ] spec 同一変更: harness.md launch 節（PBI 38 と同じ箇所の残り）+
  cli.md:424-430 delivery 節の残り
- [ ] docs-charset（U+2014 禁止）; tripwire の budget 変更は spec update と
  同一 PR

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存 headless+!verbose routing の挙動不変（:820-827）

### 統合テスト
- seam が選んだ renderer を run_headless が受けることの pin

### 単体テスト
- run_headless の最小 seam test（heartbeat/thread logic の 1 箇所 pin）

## 実装アプローチ
- **Outside-In**: 最小 seam test の Red から、renderer 選択移設 → spec 更新
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=2日。RICE 6.0。PBI 38 に依存する B slice）

## 技術的考慮事項
- 依存関係: **PBI 38 に依存**（launch 形状移設が前提。同様の data arms 追加）。
  PBI 36/37/39/40/41 と独立
- テスタビリティ: renderer 選択は data arms、heartbeat は seam test
- 非機能要件: プライバシー（fixture は合成）、analyze 側不変

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- src/cli/mod.rs: run_headless :926-1026（opencode_mod::render_run_line :1002
  直接消費、spawn/log/heartbeat :947-987）、kilo headless gate 推論 :639-647、
  headless+!verbose routing :820-827、0600 pointer 作成 :804/削除 :828
- src/adapter/harness.rs: data arms（PBI 38 で launch shape 済み、本 PBI で
  renderer 追加）
- PBI 38 の spec 更新箇所: harness.md forward-compat :222-231、launch 節
  :145-172、cli.md:424-430

### 実装手順
1. Red: run_headless の最小 seam test を書く（heartbeat が pin されない現状）
2. renderer 選択を data arms へ移設する
3. run_headless を seam が選んだ renderer を受ける形にする
4. spec 同一変更（harness.md、cli.md の残り）
5. 検証（headless routing の挙動不変込み）

### 落とし穴
- PBI 38 先行（launch 形状移設が前提。data arms の追加は同じ箇所）
- analyze 側には触れない（Harness::analyze、catalog_surfaces、guard_harness）
- docs-charset: U+2014 禁止
- heartbeat/thread logic の test は最小に（spawn 全体の test は別 PBI スケール）

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとして実装されパスする
- [ ] renderer 選択が harness seam の data arms に住む
- [ ] run_headless の最小 seam test が着地する
- [ ] `just check`（直接実行）と `cargo test` が green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] harness.md、cli.md 更新済み
