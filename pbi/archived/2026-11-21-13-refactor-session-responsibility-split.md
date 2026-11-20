# PBI: session.rs のプレゼンテーションと解決を分離する

## 優先度
- 順位: 13 / 全14件（バックログ 2026-11-21、バッチ6）
- RICEスコア: 1.3（Reach=5 / Impact=1 / Confidence=80% / Effort=3日）
- 根拠: `src/session.rs` が 1681 行（テストは 701 行目以降、実コード約 700 行）まで成長し、
  モジュール doc（src/session.rs:1-8）が謳う「One invocation's worth of resolution」を超えて
  (a) env/locale 読み取り、(b) 純粋 locale 解決、(c) CLI 解決、(d) Session open/guard/refresh、
  (e) store パス解決と Legacy ディレクトリ移行の fs I/O、(f) ANSI スタイリングと時刻表示という
  プレゼンテーションまで保有する。cli/mod.rs の行予算トリップワイヤ
  （tests/cli_mod_line_budget.rs:41 の 1135 行 / 現在 1135 行でスラック 0）を避けるため、
  2026-11-14 の言語決定表示が「shell ではなく session.rs へ」と明示的に退避しており
  （tests/cli_mod_line_budget.rs:12-16 のコメント）、予算圧力が掴みすぎを加速している。
  docs/specs/architecture.md:122 は「cli/report.rs: format, renderer, framing」と定めるが、
  framing 相当の Style は session.rs に住み続けている

## ユーザーストーリー
メンテナとして、session.rs が解決と Session の open/guard だけを持ち、プレゼンテーションは
spec が割り当てた cli/report.rs に住む状態がほしい。なぜなら掴みすぎたモジュールは次の変更
（言語決定表示のような小さな追加ですら）の退避先を失い、行予算の圧力が責務をさらに混ぜるからだ

## 背景
`src/session.rs` の責務ごとの該当箇所（行番号は 2026-11-21 時点の実コードで確認済み）:

- ANSI 基盤: src/session.rs:640-698 付近 — `Style` / `use_color`。全 view・shell 共通の
  プレゼンテーション基盤で、利用側は src/cli/ 配下に広がる
  （report.rs:7、mod.rs:17、deliver.rs:17、headless.rs:26、heartbeat.rs:18、views 6 ファイル）
- 言語決定表示: src/session.rs:181-200 付近 — `language_decision_line` /
  `eprint_language_decision`（PBI 2026-11-14-02 で shell 予算を避けてここへ退避）
- freshness 表示: src/session.rs:597-635 付近 — `eprint_freshness` / `freshness_line` /
  `humanize_age`（時刻の humanize はプレゼンテーション）
- store パス解決と Legacy 移行: src/session.rs:525-593 付近 — `store_path` /
  `migrate_legacy_store`。解決モジュール内のディレクトリ改変 fs I/O
- locale 読み取りと純粋解決: src/session.rs:47-141 付近 — `LocaleEnv` / `locale_env` /
  `process_locale_language` / `locale_is_japanese` / `locale_is_c` / `language_head` /
  `resolve_locale_language`。read-here/resolve-pure に分割済みで良い部分。
  `SessionEnv` / `EnvRoots` は :19-41 付近
- CLI 解決: src/session.rs:143-298 付近 — `ResolvedCli` / `LanguageSource` / `resolve_cli` /
  `resolve_cli_with_language` / `parse_language` / `parse_harness` / `config_file_path`
- session.rs に残すもの: src/session.rs:305-506 付近 — `Session` 構造体（:318）と
  `open` / `open_resolved` / `open_read` / `open_frozen` / `open_readonly` / `refresh` /
  `resolve_path` / `guard_harness`

## BDD受け入れシナリオ

```gherkin
Scenario: 移動は出力を一切変えない
  Given 移行前のバイナリと移行後のバイナリがある
  When 同じ transcripts・config に対して代表コマンド（doctor / waste / usage 等）を実行する
  Then stdout・stderr がバイト同一である（ANSI スタイル・freshness 行・--verbose の言語決定行を含む）

Scenario: framing 相当が spec の割当て先に住む
  Given Style / use_color / freshness 系 / language_decision 系が cli/report.rs に移っている
  When session.rs と report.rs の定義位置を確認する
  Then Style は session.rs から消え、report.rs に定義されている
  And docs/specs/architecture.md の report layout 記述（format, renderer, framing）と一致している

Scenario: 解決が session から分離し、Session は残る
  Given SessionEnv / EnvRoots / LocaleEnv / resolve_locale_language / resolve_cli 系が
    session と分離した resolve 系モジュールに移っている
  When 既存の解決テストと session.rs の残存内容を確認する
  Then Session 構造体と open / guard / refresh は session.rs に残っている
  And 既存の解決テストは移動のみで green である
```

## 受け入れ基準
- [x] `Style` / `use_color` が src/cli/report.rs へ移り、docs/specs/architecture.md:122 の
  「format, renderer, framing」割当てと一致する（spec 更新が要れば同一 PR で）
- [x] freshness 系（`eprint_freshness` / `freshness_line` / `humanize_age`）と
  language_decision 系（`language_decision_line` / `eprint_language_decision`）も
  report.rs へ移り、利用側は import 更新のみ
- [x] locale/env 解決（`SessionEnv` / `EnvRoots` / `LocaleEnv` / `resolve_locale_language` /
  `resolve_cli` 系）が session と分離した resolve 系モジュールへ移る
- [x] `Session` 構造体と open / guard / refresh は session.rs に残る
- [x] 全出力がバイト同一。移動による diff が定義位置のみであること
  （ANSI / 時刻 / 言語決定行の golden が証人）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: 移動対象の純粋関数（`resolve_locale_language`、`locale_is_japanese`、`humanize_age`
  など）の既存テストは定義に付き従う。文言のピン留め（freshness 行、言語決定行、
  エラーヒント）は 1 文字も変えない
- 統合: 既存 golden（合成ストア・transcript の E2E）が証人。diff が定義の移動のみであること
  を session.rs / report.rs / resolve 系の間で確認する
- E2E: 代表コマンドの新旧出力 diff が空（capture の diff）

## 見積もり
3日（RICE の Effort と同一根拠）

## 実装者向け注記
- **着手順**: 同日の PBI 04（advisor-language-vocabulary、src/session.rs の AdvisorLanguage
  語彙）と PBI 12（optimize-apply-prologue、refresh の pub(crate) 化）が先行着地する前提。
  **PBI 12 の後に着手すること**。PBI 07（util-layer-placement、state_base 切り出し）とも
  session.rs で接するため 07 の後でもある（バックログの依存マップ: 04 → 07 → 12 → 13）
- 最小の分割は 2 本:
  (1) プレゼンテーション（`Style` / `use_color`、freshness 系、language_decision 系）を
  cli/report.rs へ移す。spec が既に framing を report.rs に割り当てているため、利用側は
  import 更新のみ。tests/cli_mod_line_budget.rs:12-19 のコメントが「the line itself lives in
  session.rs」と述べている箇所は移動後に更新する
  (2) locale/env 解決（`SessionEnv` / `EnvRoots` / `LocaleEnv` / `resolve_locale_language` /
  `resolve_cli` 系）を session と分離した resolve 系モジュールへ移す。`Session` 構造体と
  open/guard/refresh は session.rs に残す。`refresh` 内の eprintln
  （src/session.rs:459-467 付近）は `Style` を import し直すだけで残る
- `store_path` / `migrate_legacy_store`（src/session.rs:525-593）は解決モジュール内の
  ディレクトリ改変 I/O だが、最小分割の移動対象には含めない。扱い（残置か、resolve 系への
  同行か）は実装時に判断する
- spec 同期: レイアウトが変わるため docs/specs/architecture.md（report layout リスト、
  必要なら view-landing rule 周辺）を同一 PR で更新する（.claude/rules/spec-sync.md）
- 検証コマンド: cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test
- テスタビリティ: 全出力バイト同一（ANSI / 時刻 / 言語決定行の golden が証人）。
  移動で diff が定義位置のみであること

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] 移行前後バイナリの代表コマンド出力 diff が空
- [x] コードレビュー完了（spec 同期・session.rs 接触 PBI（04 / 07 / 12）との干渉確認を含む）
- [x] ドキュメント更新済み（docs/specs/architecture.md のレイアウト記述）
