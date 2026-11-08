# PBI: E2E golden 比較を opencode / kilocode にも広げる

## ユーザーストーリー
開発者・メンテナとして、opencode / kilocode にも「旧バイナリと現行を同一合成 fixture で比較してバイト同一を確認する E2E golden」がほしい、なぜならハーネスごとの出力退行を PR で検出できるからだ。

## 優先度
- 順位: 24 / 全28件
- RICEスコア: 2.0（Reach=2 / Impact=1 / Confidence=2 / Effort=2日）
- 根拠: claude 分の記録は pbi/2026-10-29-00-backlog.md にある（旧 821bf1e バイナリと E2E 出力 269 行がバイト同一）。opencode / kilocode 分は無い（src/adapter/claude.rs のみ golden 系テストを確認済み）。opencode と kilocode のどちらを先に仕上げてもよく、参照実装は claude 分に揃える

## BDD受け入れシナリオ
Scenario: 同一合成 fixture に対する出力がバイト同一になる
  Given opencode と kilocode 各ハーネスの合成 fixture と、現行コードが生成した期待出力（golden）が用意してある
  When  analyzer にリファクタなど出力を変えない変更を加えて E2E 比較を実行する
  Then  変更前後の出力はバイト単位で同一であり、比較は green で終わる

Scenario: 意図した出力変更は golden を更新すれば red から green に移る
  Given レポートに表示項目を増やすなど意図した出力変更があり、golden はまだ旧い期待出力のまま
  When  golden を更新せずに E2E 比較を実行する
  Then  変更前の golden との差異が red として報告される
  And   golden を新しい期待出力に更新すれば、次回の実行は green に移る

## 受け入れ基準
- [x] opencode と kilocode の各ハーネスに、同一合成 fixture に対する analyze 全出力の golden があり、自動テストでバイト単位比較する（tests/e2e_golden_harness.rs＋tests/golden/2件）
- [x] claude 分と同一の手順で、変更前と現行の出力がバイト同一であることを PR で確認できる（通常cargo testで実行）
- [x] 差異が出たとき、red の報告にどの出力・どの行が違うかが分かる形で示される（assert左右表示）
- [x] 意図した出力変更では、golden を更新するだけで red から green に移る（UPDATE_GOLDEN=1。diffレビュー必須をテスト名に明記）
- [x] fixture は合成データのみで（temp内生成。DBファイル等のコミットなし）、実 transcript・実 config・生成 DB は一切コミットしない（プライバシーはハード制約）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green
- [x] docs/specs/ の該当 spec に出力退行の確認方針を同じ変更内で追記する（両harnessのsession-format specにGolden節）

## テスト戦略
- E2E: 合成 fixture を各ハーネスで analyze した全出力を golden とバイト比較する（opencode / kilocode 分。意図した出力変更の red から green への移行も含む）
- 統合: 各ハーネスの transcript と config の fixture からストア生成までが session format spec どおりに動く
- 単体: golden 更新手順（期待出力の再生成）と差分報告の形式

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- 参照実装: src/adapter/claude.rs の golden 系テスト（claude_analyze_keeps_the_recorded_counts_on_a_fixture。合成 fixture に対して変更前の analyze が記録した集計と一致することを確認、2026-10-30 記録）。ただし現状は集計値の一致までで全出力のバイト比較は自動テストに無い。旧 821bf1e バイナリとの全出力比較（E2E 出力 269 行がバイト同一）は手作業で実施した記録が pbi/2026-10-29-00-backlog.md にある
- src/adapter/opencode.rs / src/adapter/kilocode.rs には golden 系テストが無い（従来型の単体テストのみ、2026-10-31 確認済み）
- fixture は合成データのみ（.claude/rules/session-data-privacy.md）。fixture 作成の根拠は docs/specs/session-format-opencode.md / docs/specs/session-format-kilocode.md
- core は純粋で I/O フリーに保つ（.claude/rules/tdd.md）。golden 比較は E2E 層（tests/ 配下または adapter テスト）に置く
- spec 更新: docs/specs/harness.md（golden の位置づけ）と docs/specs/cli.md（比較対象の出力）を同じ変更内で更新する（.claude/rules/spec-sync.md）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（2テスト）
- [x] コードレビュー完了（親エージェントが全diff＋golden内容を目視確認）
- [x] ドキュメント更新済み（spec追記済み）
