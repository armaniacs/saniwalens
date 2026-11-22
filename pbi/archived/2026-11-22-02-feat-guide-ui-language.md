# PBI: guide TUI の対話文面を advisor language に追従させる

## ユーザーストーリー
英語環境のユーザーとして、guide TUI の対話文面が英語で表示されてほしい、
なぜならレポート本体は英語で出るのに入口の対話だけが日本語では使い始められないから。

## 優先度
- 順位: guide ローカライズ（積み残し）→ 方針決定 2026-11-22 で B 方案
  （`--language` 追従）に確定、実装へ
- RICEスコア: 1.2（Reach=3 / Impact=0.5 / Confidence=80% / Effort=1日）
  → 方針決定により Confidence が確定（解決済み言語は run_guide に既に
  流れており配線は完成、文面だけが ja 固定という不整合の解消）
- 根拠: ツール全体の言語ポリシー（レポート本体は英語、advisor 指示文は
  `--language` で切替、README は日英両版）と整合。

## 背景（現状・証拠）
- `src/cli/views/guide.rs` に日本語文字列が 20 箇所ハードコード
  （対話プロンプト 6 問・選択肢・実行表示・保存確認）。
- `run_guide` は `language: AdvisorLanguage` を既に受けている
  （呼び出し側 `cli/mod.rs` が `resolve_cli_with_language` で解決済み）。
- TUI 起動条件（`guide_entry_if_terminal`）は ja 依存なし — en ユーザーにも
  起動するため、文面だけが ja 固定という不整合が表に見える。
- 既存の語彙単一ソース化の先例: PBI 2026-11-21-04（AdvisorLanguage の
  `label_list()`）と PBI 2026-11-10-08（ガイドでも ja が効く）。

## BDD受け入れシナリオ

```gherkin
Scenario: 英語環境でガイドが英語で対話する
  Given --language en で guide を起動する
  When  対話を進める（harness 選択まで）
  Then  プロンプトと選択肢が英語で表示される
  And   実行エコーも英語で表示される

Scenario: 日本語環境では文面がバイト同一で変わらない
  Given --language ja で guide を起動する
  When  対話を進める（harness 選択から実行エコーまで）
  Then  表示文面は v0.6.8 以前とバイト同一である
  And   保存フローの確認文も ja のままである
```

## 受け入れ基準
- [x] `GuideText`（guide.rs 内）に en/ja の文面が収まり、フローコードが
  言語で分岐しない（`GuideText` を 1 度構築して渡す）
- [x] 既存の 10 E2E テスト（guide_flow.rs）は `--language ja` を明示して
  ja 文面を pin し、バイト同一を維持する
- [x] en の新規 E2E テスト（harness 選択までの文面）が追加される
- [x] guide_entry / TUI 起動条件は変更しない（terminal_available のみ）
- [x] spec 同期: docs/specs/cli.md の guide 節に文面言語の規定を追記
- [x] 台帳の guide ローカライズ（積み残し）を実施済みに更新

## テスト戦略
- E2E: guide_flow.rs のスクリプト実行（ja pin + en 新規）
- 単体: GuideText の en/ja 文言 pin（Cursor-driven テストの既存形に合わせる）

## 見積もり
1日

## 実装者向け注記
- ja 文言は現行の文字列を 1 文字も変えない（バイト同一が pin の前提）。
- en 文言は本 PBI で設計（表記は report 側の英語トーンに合わせる）。
- `ask_profile_name` / `ask_choice` / `ask_yes_no` の I/O 汎用シグネチャは
  変更しない（文言は呼び出し側から渡す）。

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] e2e golden 3/3（バイト同一の証人）が green
- [x] コードレビュー完了
- [x] spec 同期済み（cli.md guide 節）
- [x] 台帳の guide ローカライズ（積み残し）を実施済みに更新
