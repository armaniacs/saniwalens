# PBI: リリースノート自動化（CHANGELOG 下書き生成）を用意する

## ユーザーストーリー
開発者・メンテナとして、git-cliff 等で Conventional Commits から CHANGELOG の下書きセクションを生成する仕組みがほしい、なぜなら手書き CHANGELOG の起筆コストを下げられるからだ。

## 優先度
- 順位: 26 / 全28件
- RICEスコア: 2.0（Reach=2 / Impact=1 / Confidence=2 / Effort=2日）
- 根拠: 手書き運用は機能しているため、下書き生成の位置づけ。ツール（git-cliff 等）の選定は discovery で決める

## BDD受け入れシナリオ
Scenario: タグからそのリリースの下書きが生成される
  Given Conventional Commits 形式のコミット履歴と、切り出したリリースのタグがある
  When  タグを渡して下書き生成を実行する
  Then  そのリリースに含まれる変更がカテゴリ（Added / Changed / Fixed など）ごとに整理された CHANGELOG セクションの下書きが出る

Scenario: 形式外のコミットメッセージの扱いが決まっている
  Given Conventional Commits 形式に従わないコミットメッセージが履歴に含まれる
  When  下書き生成を実行する
  Then  形式外のメッセージは Other に集められるか、下書きから除外される。どちらの挙動かは設定で明示される

## 受け入れ基準
- [ ] タグを渡すと、そのリリースに含まれる変更の CHANGELOG セクション下書きが生成される
- [ ] Conventional Commits 形式外のメッセージは Other に集められるか除外される（設定で明示）
- [ ] 生成物は Keep a Changelog 形式の既存セクション構成（Added / Changed / Fixed など）に沿う
- [ ] 生成物の文字種は CP932 範囲内で、既存 CHANGELOG と同じ基準に揃える
- [ ] 自動生成を commit するか下書きに留めるかを discovery で決定し、手順に反映する
- [ ] 既存の手書き CHANGELOG を壊さない（記録対象はオリジナル cclens からの差分のみという既存方針も含む）
- [ ] 検証（cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test）が green

## テスト戦略
- E2E: 実履歴（または合成履歴）から下書き生成を実行し、セクション構成と文字種を確認する
- 統合: タグとコミット範囲の指定が生成内容に正しく反映される
- 単体: 形式外メッセージの分類（Other 集約か除外か）、コミット種別とカテゴリの対応（feat が Added、fix が Fixed など）

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- CHANGELOG.md は Keep a Changelog 形式（ja 1.1.0）で手書きが続いており、先頭に記録対象はオリジナル（lambdalisue/cclens v0.4.0）からの差分のみという方針が明記されている。この方針を壊さない
- Conventional Commits の規約は .claude/rules/commit-convention.md にあるが、CI での lint は無い（2026-10-30 backlog 記録）。形式外メッセージの存在は前提として設計する
- git-cliff 等のツール選定と、生成物を commit するか下書きに留めるかは discovery で決める
- 生成物は CP932 文字種。docs-charset のチェック（scripts/check_docs_chars.py 相当）を生成物にも適用する
- spec 更新: コード変更が無い（ツール設定と手順のみ）場合は spec 更新は不要。CI に組み込む場合はその挙動を README または docs に記す

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
