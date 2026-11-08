# PBI: PR テンプレートを置く

## ユーザーストーリー
開発者・メンテナとして、動機 / 検証 / 確認項目の骨格を新規 PR に自動で用意する仕組みがほしい、なぜなら PR 説明の抜け漏れを防ぎ、レビューの往復を減らしたいから（現状は docs/pr/ の日英 PR 文を手書きしている）。

## 優先度
- 順位: 10 / 全28件
- RICEスコア: 6.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: プロセスの一貫性。既存の docs/pr/ の運用を壊さず骨格だけ与えるので確信度は高い。09（SECURITY / CONTRIBUTING）と独立。

## BDD受け入れシナリオ
Scenario: 新規 PR の本文がテンプレートから始まる
  Given PR テンプレートがリポジトリに置かれている
  When  開発者が新規 PR を作成する
  Then  PR 本文がテンプレートの骨格（動機 / 検証 / 確認項目）で初期化される

Scenario: 検証手順の節に just check / just test の記載がある
  Given テンプレートに検証手順の節がある
  When  テンプレートの内容を実際の検証コマンドと突き合わせる
  Then  検証手順の節に just check と just test の記載があり、実際に green になる手順と一致する

## 受け入れ基準
- [x] .github/PULL_REQUEST_TEMPLATE.md が新規作成されている
- [x] 骨格は 動機 / 検証 / 確認項目 の 3 節を持つ
- [x] 検証手順の節に just check と just test の記載がある
- [x] docs/pr/ の日英 PR 文の運用と矛盾しない（骨格のみ。手書き運用は置き換えない）（テンプレートは共通骨格であり、docs/pr/ の手書き運用は置き換えない）
- [x] テンプレートは docs-charset 規約（check_docs_chars.pyで適合確認）（CP932 で表現できる文字のみ）に適合する
- [x] 既存ワークフローや既存文書の内容は変わらない

## テスト戦略
- E2E: 新規 PR 作成画面を開き、本文がテンプレートで初期化されることを確認する
- 統合: テンプレートの節構成と docs/pr/ の既存 PR 文（.ja.md / .en.md）の構成を突き合わせ、共通骨格として矛盾が無いことを確認する
- 単体: 文字種チェック（docs-charset の検証スクリプト）と、見出し・節の構成確認

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 新設: .github/PULL_REQUEST_TEMPLATE.md（新規ファイルのみ）。既存の docs/pr/ は日英ペア（.ja.md / .en.md）を手書きする現行運用なので、テンプレートは共通骨格に留め、docs/pr/ の運用は置き換えない
- 制約: CP932 文字種（docs-charset 規約）。見出しは日英併記でもよいが、使う文字は範囲内に留める。em dash U+2014 は使わない
- spec 更新: 不要
- 関連: .github/PULL_REQUEST_TEMPLATE.md（新設）、docs/pr/ の .ja.md / .en.md、justfile
- 落とし穴: テンプレートを長くしすぎると記入が形骸化する。動機 / 検証 / 確認項目の 3 節に留める

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（E2EのGitHub画面確認は初回PR作成時に目視 — 外部。構成突合は目視済み）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（本件自体が文書。spec更新不要）
