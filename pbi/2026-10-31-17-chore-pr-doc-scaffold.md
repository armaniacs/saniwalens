# PBI: docs/pr/ の日英 PR 文スキャフォールドを用意する

## ユーザーストーリー
saniwalens の開発者・メンテナとして、slug を渡すと `docs/pr/<slug>.en.md` と `docs/pr/<slug>.ja.md` の雛形を生成するスクリプトがほしい、なぜなら PR 文の手書き運用の脱漏（日英どちらかが欠ける・そもそも書かれない）を防げるからだ。

## 優先度
- 順位: 17 / 全28件
- RICEスコア: 3.0（Reach=1 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: 小さな自動化で、docs/pr/ の日英 7 組の手書き運用を標準化する。依存: なし。

## BDD受け入れシナリオ
Scenario: slug を渡すと日英 2 ファイルの雛形が生成される
  Given docs/pr/ に同名の雛形ファイルが存在しない
  When  開発者が slug を指定して雛形生成を実行する
  Then  `docs/pr/<slug>.en.md` と `docs/pr/<slug>.ja.md` の 2 ファイルが生成される
  And   雛形には既存 PR 文の骨格（追加するもの・背景・検証の節）と、うめ込む箇所の印が載る

Scenario: 既存ファイルは上書きされずエラーになる
  Given `docs/pr/<slug>.en.md` または `docs/pr/<slug>.ja.md` のどちらかが既に存在する
  When  開発者が同じ slug を指定して雛形生成を実行する
  Then  雛形の生成は行われず、既存ファイルがある旨のエラーになる
  And   既存ファイルの内容は一切変わらない（上書き・部分書き換えが無い）

## 受け入れ基準
- [ ] slug を渡すと `docs/pr/<slug>.en.md` と `docs/pr/<slug>.ja.md` が 1 回の実行で 2 ファイル生成される
- [ ] 既存ファイルが 1 つでもある slug は、上書きせずエラーで終了する（両方未存在のときだけ生成する）
- [ ] 雛形の骨格は既存 PR 文（docs/pr/rename-saniwalens.ja.md と .en.md など）の節立てを踏襲する
- [ ] 生成される文書は docs-charset 規約（CP932 で表現できる文字のみ）に従い、雛形自体が範囲外文字を混入させない
- [ ] 引数なしの実行では使い方を表示する（誤って空 slug でファイルを作らない）
- [ ] 既存の検証コマンド（cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test）の結果を変えない

## テスト戦略
- E2E: 新しい slug で実行して 2 ファイルが生成されること、既存 slug で実行してエラーになり内容が変わらないことをユーザーシナリオとして確認する
- 統合: 生成した雛形 2 ファイルが docs-charset の文字種検証（check_docs_chars.py）を通ることを確認する
- 単体: slug の正規化（前後の空白・区切り文字）、引数なし・空 slug の扱い、既存ファイル検出の境界（片方だけ存在・両方存在）をテストする

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 配置先は現状 scripts/ が無いため実装者に任せる（scripts/ 新設か、justfile の recipe か）。docs/pr/ の運用が手書きである現状の脱漏防止が目的
- 雛形の骨格は既存 PR 文を踏襲する: docs/pr/ の 7 組（rename-saniwalens、version-from-git、period-filter、feature-kilocode、feature-otherharness、rename-agentlens、drop-plugin-generalize-copy）の共通構造を確認してから節立てを決める
- 生成物の文字種: docs-charset スキルの check_docs_chars.py で検証し exit 0 を確認する。雛形に em dash（U+2014）・範囲外記号・アクセント付きラテン文字・絵文字を含めない
- 上書き防止はスクリプト内で既存ファイルの存在確認を行い、テストで上書きの無いことを確認する
- spec 更新: 不要（CLI の動作変更でない）。docs/specs/ に PR 文の規約は無い
- プライバシー: 雛形はプレースホルダのみ。実 PR の内容・実データは含めない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
