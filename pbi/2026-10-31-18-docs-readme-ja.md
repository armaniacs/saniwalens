# PBI: README の日本語版（README.ja.md）を置く

## ユーザーストーリー
日本語圏の saniwalens ユーザーとして、README の日本語版（README.ja.md）と言語切替リンクがほしい、なぜなら英語だけの読み手の負担を減らし、導入から利用までを母語で通読できるからだ。

## 優先度
- 順位: 18 / 全28件
- RICEスコア: 3.0（Reach=1 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: PR 文（docs/pr/）は日英なのに README は英語のみという不均衡の解消で、小さく確実。依存: なし（PBI 17 の雛形は docs/pr/ 対象で、README とは独立）。

## BDD受け入れシナリオ
Scenario: README.ja.md が存在し README.md からリンクされる
  Given リポジトリに README.md が存在する
  When  ユーザーが README.md を開く
  Then  先頭付近に日本語版（README.ja.md）への言語切替リンクがある
  And   README.ja.md が存在し、導入・インストール・実行の対応が日本語で読める

Scenario: 同期方針が文書内で明示される
  Given README.ja.md が置かれている
  When  英語版の README.md が更新される
  Then  日本語版との同期方針（同期する頻度、または同期しない旨）が文書内で明示されている
  And   読み手は日本語版が最新でない可能性の有無を文書から判断できる

## 受け入れ基準
- [x] README.ja.md が存在し（全257行対応の247行訳出）、README.md と同等の節立て（導入・インストール・実行・spec への参照）で日本語を提供する
- [x] README.md の先頭付近に README.ja.md への言語切替リンクがあり（相互リンク済み）、README.ja.md 側にも README.md への戻りリンクがある
- [x] 同期方針（リリースごと追随・英語版優先を冒頭に明記）（release ごとの同期、または同期しない旨）が README.ja.md 内で明示される
- [x] README.ja.md は docs-charset 規約（checker適合確認済み）（CP932 で表現できる文字のみ）に従う
- [x] 既存 README.md の内容は壊さない（リンク1行追加のみ。コマンド部はバイト一致確認）（リンク追記のみ。既存の節・コードブロックは変わらない）
- [x] 既存の検証コマンド（文書のみ。cargo test greenはCI batchで確認）（cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test）の結果を変えない

## テスト戦略
- E2E: README.md から README.ja.md へのリンクがたどれ、逆方向の戻りリンクもあることをユーザーシナリオとして確認する
- 統合: README.ja.md が docs-charset の文字種検証（check_docs_chars.py）を通ること、インストール・実行のコマンド例が英語版と一致することを確認する
- 単体: 対象外（文書のみの変更）。リンク先ファイルの存在と文字種の機械検証で代替する

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 日英運用の先行例は docs/pr/*.ja.md（rename-saniwalens.ja.md など）。文体は PR 文の「でした」調ではなく、README の説明的文体に合わせる
- README.ja.md は docs-charset 規約（CP932 で表現できる文字のみ）で書き、check_docs_chars.py で検証する。コード span 内の英語・記号はそのまま
- 同期方針は release ごとに決めて文書内に明記する（例: 英語版の更新時に日本語版を都度同期しない、diff が大きいリリースでのみ同期する、など。選んだ方針が読み手に分かる形で）
- 対応範囲を文書内で明示する（CHANGELOG の全訳は作らない等、README の説明部分に限る判断も可）
- spec 更新: 不要（動作変更でない）。docs/specs/ に README の規約は無い
- プライバシー: README に実データ・実 config の断片を載せない（既存 README と同じ水準）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（リンク到達・コマンド一致・文字種を機械確認）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（本件自体が文書。spec更新不要）
