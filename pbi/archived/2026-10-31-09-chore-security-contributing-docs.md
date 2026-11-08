# PBI: SECURITY.md と CONTRIBUTING.md を置く

## ユーザーストーリー
外部コントリビュータ・脆弱性報告者として、受入口の文書がほしい、なぜなら報告経路と参加方法が明確になり、迷わず安全に貢献できるから。

## 優先度
- 順位: 09 / 全28件
- RICEスコア: 6.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: OSS としての衛生で、外部参加の受入口を整える。内容は正確さが命なので内容レビューを主に置く。10（PR テンプレート）と独立。

## BDD受け入れシナリオ
Scenario: ルートに 2 文書が存在し README からリンクされる
  Given リポジトリのルートに README がある
  When  脆弱性の報告方法と参加方法を README から辿る
  Then  ルートに SECURITY.md と CONTRIBUTING.md の 2 文書があり、README から両方へリンクが張られている

Scenario: CONTRIBUTING.md が実際の開発フローと一致する
  Given CONTRIBUTING.md に開発フローの説明がある
  When  文書の内容を実際の開発手順と突き合わせる
  Then  nix dev shell、just check / just test、Conventional Commits、PBI-first、BDD/TDD の記述が実際の運用と一致し、実在しない手順が書かれていない

Scenario: 報告経路が実在する
  Given SECURITY.md に脆弱性の報告経路が書かれている
  When  その経路に従って報告しようとする
  Then  経路は実在する（GitHub の非公開の脆弱性報告など）。リンク切れや虚偽の経路が書かれていない

## 受け入れ基準
- [x] ルートに SECURITY.md と CONTRIBUTING.md がある（CONTRIBUTINGは既存。SECURITY.md＋SECURITY.ja.mdを新設）
- [x] README から 2 文書へリンクが張られる（README.mdにSECURITYリンク追加）
- [x] SECURITY.md の報告経路は実在するものだけを書く（GitHub Security Advisories。連絡先捏造なし）（虚偽の連絡先・リンク切れが無い）
- [x] CONTRIBUTING.md は実際の開発フロー（nix/just/make、CI、PBI-first、BDD/TDD記述と一致確認済み）（nix dev shell、just check / just test、Conventional Commits、PBI-first、BDD/TDD）と一致する
- [x] docs/specs/*.md と .claude/rules/*.md の記述と矛盾しない（目視確認）
- [x] プライバシーの姿勢（CONTRIBUTING既存記述＋SECURITY報告節で合成例要求。目視確認）（fixture は合成、実 transcript・実 config をコミットしない）が参加手順の中で伝わる
- [x] 2 文書は docs-charset 規約（check_docs_chars.pyで適合確認）（CP932 で表現できる文字のみ、em dash U+2014 禁止）に適合する

## テスト戦略
- E2E: README からリンクを辿って 2 文書に到達できること
- 統合: 文書の内容を実際のリポジトリ（justfile、.github/workflows/、.claude/rules/、docs/specs/）と突き合わせ、齟齬が無いことを確認する
- 単体: 文字種チェック（docs-charset の検証スクリプト）とリンク切れの確認

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 制約: 文書なので BDD より内容レビューが主。正確さが命（実在しない経路・手順を書かない）。内容の主な突き合わせ先: justfile（check / test の定義）、.github/workflows/ci.yml（CI の実体）、.claude/rules/commit-convention.md（Conventional Commits）、AGENTS.md（PBI-first、TDD、privacy、subagent-driven）、docs/specs/*.md
- privacy: 例示に実データを使わない。参加手順には合成 fixture と privacy 硬制約（.claude/rules/session-data-privacy.md）を反映する
- spec 更新: docs/specs/*.md は変更不要（動作変更でない）。README へのリンク追記はこの PBI の範囲
- 関連: ルートに SECURITY.md / CONTRIBUTING.md を新規作成、README.md にリンク追記
- 落とし穴: 新規ルート文書は crate の tarball に含まれる（Cargo.toml の exclude は .github/ や AGENTS.md を対象にするが、新ファイルは未指定）。パッケージに含めるか除外するかは Cargo.toml のコメントに書かれたポリシー（furniture は出さない、README から辿れる文書は出す）に合わせて決める

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（リンク到達・内容突合・文字種を目視＋機械確認）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（本件自体が文書。spec更新不要）
