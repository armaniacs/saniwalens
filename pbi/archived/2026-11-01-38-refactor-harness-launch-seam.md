# PBI: launch 形状を harness seam の data arms に移す（新 harness 契約を launch 側でも構造で充足する）― A slice

## ユーザーストーリー
実装者として、harness ごとの launch 形状（binary/argv/run mode）が harness seam
の data arms に住んでいてほしい、なぜなら新 harness 契約（「variant + adapter
module で cli/mod.rs は変更しない」）が analyze 側でしか構造で充足されず、launch
側は compile error の破局的強制と discipline でしか保たれていないからだ。

## 優先度
- 順位: 38 / 全候補数 7
- RICEスコア: 12.0（Reach=4 / Impact=2 / Confidence=3 / Effort=2日）
- 根拠: 新 harness 追加の影響パスが最も広い（R=4）。stale comment（:3099-3104、
  assertion と spec 矛盾）が実在する実証付き。PBI 42（B slice: renderer 選択
  移設）に先行

## ビジネス価値
- 新 harness が adapter module + variant だけで着地する（launch 側を含む）。
  harness.md の契約が analyze と launch の両側で構造で充足される
- 測定方法: launch shape が adapter/harness.rs の data arms に住み、
  launch_command_runs_each_harness_own_agent が seam を test すること

## 背景（なぜなぜ分析 2026-11-01 round3、3 立場 x 30 段階、file:line 実証）
- Harness seam（adapter/harness.rs）は per-harness 事実を data arms
  （~20 match arms）として集約するが、その集約が analyze/reporting 側に限られ、
  launch 形状（binary/argv/run mode）と headless renderer 選択は shell
  （cli/mod.rs launch_command :629-649 / run_headless :926-1026）に
  implementation として住む
- 新 harness 契約（harness.md:222-231、harness.rs:210-215）は analyze に
  ついてのみ構造で充足され、launch 側は compile error の破局的強制
  （:632-648 wildcard 無し match）と discipline で保たれる
- archived PBI 06 が launch 側集約を明示 defer した（:10-12）が、defer 条件
  （次の起動 quirk）を検知する構造が無い
- **stale comment cli/mod.rs:3099-3104** は assertion（:3117-3124）と spec
  （harness.md:165-172）に矛盾する記述 ― 監査視点の最大証拠
- kilo は OpenCode fork なので headless kilocode も seam の内側で自然に成立する
  （code comment :639-647 は既に headless gate を推論している）

## BDD受け入れシナリオ

```gherkin
Scenario: launch shape は harness seam の data arm である
  Given adapter/harness.rs に launch shape の per-harness 事実がある
  When 新 harness を variant + adapter module で追加する
  Then cli/mod.rs の launch 側は変更不要である（契約が launch 側でも真になる）

Scenario: seam test は shell でなく seam を test する
  Given launch_command_runs_each_harness_own_agent の pin 値がある
  When test を実行する
  Then test は adapter/harness.rs の data arms を通って pin 値を確認する

Scenario: 既存の analyze 側は変わらない
  Given run_analyze → guard_harness の順序がある
  When 本 PBI を着地させる
  Then analyze 側の契約・adoption rule は不変である
```

## 受け入れ基準
- [x] launch_command の launch shape（binary/argv/RunMode）を
  adapter/harness.rs の data arms（input_unit/copy/db_file_name の隣）へ移設する
- [x] analyze 側に触れない: Harness::analyze（:216-222）/ catalog_surfaces
  （:186-208）と run_analyze → guard_harness の順序（cli/mod.rs:2158-2159、
  harness.md:202-204）を変えず、guard_harness の adoption rule
  （session.rs:229-235、test :645-657）を保つ
- [x] launch_command_runs_each_harness_own_agent（:3098-3125）の pin 値
  （kilo=("kilo",["--prompt"],Interactive) 等）をそのまま seam test へ写す
- [x] stale comment :3099-3104 を同一変更で修正する（assertion :3117-3124 と
  harness.md:165-172 に矛盾する記述）
- [x] AdvisorLaunch/deliver の test double（RecordingAdvisor :3132-3183、
  0600 pointer 作成 :804/削除 :828、headless+!verbose routing :820-827）は
  現行どおり
- [x] B slice（renderer 選択移設 + run_headless heartbeat seam test）は
  PBI 42 として分離する
- [x] spec 同一変更: harness.md forward-compat :222-231（"cli/mod.rs gains no
  dispatch arm" :227 / "only harness-aware step" :228-229 の書き換え）+
  launch 節 :145-172 へ知識の帰属追記 + cli.md:424-430 delivery 節 +
  harness.rs:210-215 doc
- [x] docs-charset（SHIFT-JIS/CP932、範囲外文字を持ち込まない）。
  tripwire :16（3,383 無余裕）の budget 変更は spec update と同一 PR
  （mod.rs は縮む方向）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存 launch_command_runs_each_harness_own_agent の挙動不変を pin

### 統合テスト
- seam test（data arms を通る launch shape pin）

### 単体テスト
- adapter/harness.rs の launch shape 腕ごとの pin 値

## 実装アプローチ
- **Outside-In**: seam test（pin 値の移設）から Red、launch shape 移設で
  Green、spec 更新で仕上げる
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=2日。RICE 12.0）

## 技術的考慮事項
- 依存関係: PBI 42 が依存（launch 形状移設が前提）。PBI 40 と同じ cli/mod.rs
  触りのため直列（本 PBI 先行）。PBI 36/37/39/41 と独立
- テスタビリティ: launch shape は data arms、純粋 pin 可能
- 非機能要件: プライバシー（fixture 無し）、analyze 側不変

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- src/cli/mod.rs: launch_command :629-649（kilo の headless gate 推論 :639-647
  込み）、run_headless :926-1026（opencode_mod::render_run_line :1002 直接
  消費）、launch pin test :3098-3125、stale comment :3099-3104、RecordingAdvisor
  :3132-3183
- src/adapter/harness.rs: data arms（~20 腕）、Harness::analyze :216、
  catalog_surfaces :186、新 harness 契約 doc :210-215
- src/session.rs: guard_harness :229-235（adoption rule）、test :645-657

### 実装手順
1. Red: seam test を書く（launch shape を adapter/harness.rs から取る現状 pin）
2. launch shape を data arms へ移設する（input_unit/copy/db_file_name の隣）
3. stale comment :3099-3104 を修正する
4. spec 同一変更（harness.md、cli.md、harness.rs doc）
5. 検証（analyze 側不変の確認込み）

### 落とし穴
- wildcard 無し match の破局的強制（:632-648）を保つ（wildcard を足さない）
- run_headless heartbeat（:947-987）は tests/ で 0 件（probe 実測）―
  PBI 42 のスコープ、本 PBI では触れない
- docs-charset: 範囲外文字（em dash 等）を持ち込まない
- PBI 40（flag surface 集約）と同じ cli/mod.rs 触り ― 本 PBI 先行で直列

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] launch shape が adapter/harness.rs の data arms に住む
- [x] `just check`（直接実行）と `cargo test` が green（analyze 側不変込み）
- [x] コードレビュー完了（task review + 最終 review）
- [x] harness.md、cli.md、harness.rs doc 更新済み（stale comment 修正込み）
