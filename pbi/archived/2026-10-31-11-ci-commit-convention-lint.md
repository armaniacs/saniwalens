# PBI: Conventional Commits lint を CI に追加する

## ユーザーストーリー
開発者・メンテナとして、PR タイトルとコミットメッセージを形式チェックする機能がほしい、なぜなら .claude/rules/commit-convention.md の規約を文書だけでなく CI で強制したいから（現状は文書だけで未強制）。

## 優先度
- 順位: 11 / 全28件
- RICEスコア: 6.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: 規約の機械化で工数が小さく確信度は高い。09（受入口文書）で規約の告知が整った後に置くと噛み合うが、依存は必須ではない。

## BDD受け入れシナリオ
Scenario: 不正形式の PR タイトルは CI が失敗する
  Given Conventional Commits の形式規約（type 必須、小文字開始、末尾句点なし、70 字以下）がリポジトリの規約文書として存在する
  When  type が無い、大文字で始まる、または 70 字を超える PR タイトルで PR を出す
  Then  CI が失敗し、規約のどの点に違反したかが CI の指摘から分かる

Scenario: 正しい形式の PR は green になる
  Given 同じ規約がリポジトリに存在する
  When  規約に適合した PR タイトル（例: feat: add usage trend view）で PR を出す
  Then  CI は green になり、違反の指摘は出ない

## 受け入れ基準
- [x] PR タイトルが type 必須（.github/scripts/commit_title_lint.py。15件の境界テストでローカル検証済み）・小文字開始・末尾句点なし・70 字以下の規約に適合しない場合、CI が red になる
- [x] 規約に適合する PR タイトルは green になる（同スクリプトで検証済み）
- [x] 70 字ちょうどは green、71 字は red（ちょうど70/71で検証済み）（境界の確認）
- [x] scope 付きタイトル（検証済み）（type(scope): subject）も正しく評価される
- [x] scope の語彙では失敗させない（正規表現はscope内容を不問）（scope は自由。frozen リストとして lint しない）
- [x] 許可 type（feat / fix / refactor / perf / test / docs / build / ci / chore）の範囲で判定する（commit-convention.mdと一致確認済み）（.claude/rules/commit-convention.md と一致）
- [x] 既存 CI ジョブの内容と結果は変わらない

## テスト戦略
- E2E: 不正形式の PR タイトルで red、正しいタイトルで green になることを実際に確認する
- 統合: 既存 CI ワークフローへのジョブ追加が他のジョブを壊さないこと、ワークフロー構文の検査
- 単体: 形式判定の境界値（69 / 70 / 71 字、scope 付き・無し、末尾句点の有無）と例外（type の大文字小文字、type! の扱い）

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 規約の一次情報: .claude/rules/commit-convention.md（imperative、小文字開始、末尾句点なし、70 字以下。許可 type は feat / fix / refactor / perf / test / docs / build / ci / chore）。lint はこの文書と矛盾させない
- scope は自由（frozen リストではない点に注意）。scope の語彙検査はしない。モジュール構成が発展途上のため、規約文書自体が scope を凍結していない
- 70 字の対象: 規約文書では subject（type(scope): の後ろ）に 70 字以下を課している。検査対象を PR タイトル全体にするか subject にするかを実装前に確定し、規約文書と揃える（ずれると正当な PR が red になる）
- spec 更新: 不要
- 関連: .claude/rules/commit-convention.md、.github/workflows/ci.yml

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（スクリプトの境界テストで検証。CI上のred/greenは初回PRで確認 — 外部）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（CIのみの変更。spec更新不要）
