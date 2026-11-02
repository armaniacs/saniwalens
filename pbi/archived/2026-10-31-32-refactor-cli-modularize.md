# PBI: cli.rs をモジュールに分割する（trend から 1 移動ずつ）

## ユーザーストーリー
実装者として、CLI のビューがモジュールに分かれていてほしい、なぜなら
cli.rs が 4,570 行まで育ち、新ビューのたびに追記が最短経路になり移動量が
複利で増えるからだ。

## ビジネス価値
- 以降のビュー追加が「cli/views/ に 1 ファイル追加」になり、機能追加と
  移動の混在 PR を生まない
- 測定方法: 出力が従来とバイト同一（capture と合成ゴールデンで検証）、
  `just check` と `cargo test` が green、テスト名一覧の前後 diff が空

## 背景（2026-10-31 のレビュー持ち越し指摘4、なぜなぜ分析の実証）
- cli.rs は 4,570 行（コード 3,358 + テスト 1,212 / 57 fn）。4.5 ヶ月で
  161 行（06-15）から 4,570 行（10-31）に成長。直近の契機は 6d64d9e（+609）
  と d4b1776（+842）。trend 専用部は cli.rs:1713-2122 の約 410 行
- 根本原因（構造）: `lib.rs` の no-scaffolding 規約の保守的解釈により、
  新ビューの追記が cli.rs に集中した。adapter（7 ファイル）と core（14
  ファイル）は既に分割済みで CLI だけ未分割。architecture.md は report 層を
  定義（docs/specs/architecture.md:96-101）するが src/ に report モジュール
  が無い
- 手続き的仮定 3 つは反証済み: (1) spec 発火は最小（必要な spec 変更は
  architecture.md 層表の report 行 1 箇所のみ）(2) E2E キャプチャは不要
  （プライバシーで非コミット。tests/trend_markdown.rs 型の合成ゴールデンが
  コミット可能・CI 検証可能で、trend は既に所持）(3) 「次のビュー追加まで
  待つ」は有害（混在 PR を生み、移動量が複利で増える）
- 前例: `77e5621` の session.rs 約 1,000 行抽出（1 コミット＝ 1 移動）

## BDD受け入れシナリオ

```gherkin
Scenario: 出力は一切変わらない
  Given 移動前の全ビュー・trend の出力を保存してある
  When モジュール分割を行う
  Then 全ビュー・trend の出力はバイト同一である
  And just check と cargo test は green のままである

Scenario: 以降のビュー追加は views/ に入る
  Given cli/views/ が存在する
  When 次のビューを追加する
  Then 新しいビューは cli/views/ の 1 ファイルになり、cli.rs への追記は
  配線 1 箇所で足りる
```

## 受け入れ基準
- [x] `src/cli/trend.rs` を抽出する（trend 専用部 cli.rs:1713-2122 の約
  410 行、他ビューから呼ばれない部分）。trend は合成ゴールデン
  （tests/trend_markdown.rs 型）を所持済みでリグレッション検証可能
- [x] `src/cli/window.rs` を抽出する（window_line / window_json /
  eprint_window / framed 等の 8〜11 ビュー共有部、約 90 行）
- [x] `src/cli/report.rs` を抽出する（render / preamble 等の report 層の
  実体、約 150 行）。残りは `src/cli/mod.rs`
- [x] 1 コミット＝ 1 移動（`77e5621` の前例）。テストは移動先 mod に従属
  （`.claude/rules/tdd.md:41`）、テスト名一覧の前後 diff が空であることを
  検証する（テストは移動のみで改名しない）
- [x] semver: 新モジュールは `pub(crate)` 配下に置き、公開 API（run / Cli /
  DeliveryMode / DeliveryPlan）は mod.rs で再輸出する。
  cargo publish（Cargo.toml）への影響なし
- [x] spec 更新: `docs/specs/architecture.md` 層表の report 行 1 箇所のみ
  （src/ に report モジュールが実在した旨）
- [x] 以降のビュー追加は `src/cli/views/` に 1 ビュー 1 ファイルである旨を
  受け入れ基準として明文化する（views/ は次のビュー追加時に作る）
- [x] `just check` と `cargo test` が green（各移動のたび）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 合成ゴールデン（tests/trend_markdown.rs 型）と capture の diff で出力不変

### 統合テスト
- 移動前後のテスト名一覧 diff が空（テストは移動のみで改名しない）

### 単体テスト
- 既存の単体テストが移動先 mod に従属して green

## 実装アプローチ
- **リファクタリング**: 振る舞いを変えず、1 移動ごとに検証
  （Green のまま移動する）。テストは先に移動先に従属させ、 Red にしない
- 実装は subagent-driven-development スキルで実施する

## 見積もり
2 SP（E=2日。RICE 6.0: R=2 / I=2 / C=3 / E=2日）

## 技術的考慮事項
- 依存関係: PBI 29 / 30（cli.rs の trend 表示部に触れる）の**後に実施する**。
  移動と機能変更の混在を避けるため
- テスタビリティ: 移動のみで振る舞いは変わらない。ゴールデンと capture が
  リグレッションの網
- 非機能要件: プライバシー（capture は非コミット、ゴールデンは合成のみ）、
  crates.io 公開 API の不変

## 実装者向け注記

### 現状コードの確認（2026-10-31 実施済み）
- `src/cli.rs` の構造: コード 3,358 行 + テスト 1,212 行（57 fn）。
  trend 専用部 1713-2122、期間表示部 1623-1712、report 層の実体
  3278-3358 + preamble 等
- `src/lib.rs` の公開項目（run / Cli / DeliveryMode / DeliveryPlan）と
  no-scaffolding 規約（lib.rs:5-6 付近）
- `docs/specs/architecture.md` の層表（96-101 行目付近）

### 実装手順
1. `src/cli/trend.rs` を抽出する（テストを先に従属させ、テスト名一覧の
   diff が空であることを確認してから移動する）
2. `cargo test` → `just check` → ゴールデンで出力不変を確認する
3. `src/cli/window.rs` を抽出する（共有部のみ、trend 専用部は含めない）
4. `src/cli/report.rs` を抽出する（render / preamble）
5. `src/cli/mod.rs` に公開 API の再輸出を置く
6. `docs/specs/architecture.md` の層表 report 行を更新する

### 落とし穴
- 移動と機能変更を 1 コミットに混ぜない（レビュー不能になる）。
  1 移動＝ 1 コミット、機能変更は別 PBI
- `pub use` の再輸出漏れで crates.io の公開 API が変わる。
  `cargo publish --dry-run` または semver-checks の目を通す
- テストを改名すると名前一覧の diff が空でなくなり、リグレッションの
  検証が弱くなる。改名は移動と分けない

## Definition of Done
- [x] 全移動が完了し、出力がバイト同一（ゴールデン + capture）
- [x] テスト名一覧の前後 diff が空
- [x] `just check` と `cargo test` が green
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/architecture.md 更新済み（report 層）
