# PBI: PR preview build（artifact 添付）を CI に追加する

## ユーザーストーリー
開発者・メンテナ、レビュアーとして、PR に preview バイナリを artifact として添付する仕組みがほしい、なぜならレビュアーが実機で動作を試せるからだ。

## 優先度
- 順位: 28 / 全28件
- RICEスコア: 1.0（Reach=1 / Impact=1 / Confidence=2 / Effort=2日）
- 根拠: 付加価値は低いが reviewer 体験は上がる。.github/workflows/build.yml（workflow_call、matrix ビルド）を再利用するため実装コストは低い

## BDD受け入れシナリオ
Scenario: PR で preview バイナリが添付される
  Given PR が作成または更新されている
  When  CI が matrix ビルドを実行する
  Then  各プラットフォームの preview バイナリ（アーカイブ）が artifact として upload され、レビュアーがダウンロードして実機で試せる

Scenario: ビルドが失敗したらジョブが red になる
  Given コンパイルが通らない変更が PR に含まれる
  When  CI が preview build を実行する
  Then  失敗したジョブは red になり、PR がその旨を示す

## 受け入れ基準
- [ ] PR で preview バイナリが artifact として upload される
- [ ] matrix ビルド（.github/workflows/build.yml を再利用）の対象プラットフォームが preview にも適用される
- [ ] ビルドが失敗したらジョブが red になる
- [ ] artifact の名前から PR と対象プラットフォームが分かる
- [ ] release の秘密情報には触れない（リリース用の署名・token を preview では使わない）
- [ ] 既存の CI ジョブ（check / test / build / targets / package）の挙動を壊さない
- [ ] 検証（cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test）が green

## テスト戦略
- E2E: PR を出して artifact が upload されダウンロードできること。ビルドが失敗する変更でジョブが red になること
- 統合: build.yml の workflow_call が preview 用の入力で正しく動く
- 単体: artifact 名の命名規則（PR 番号と対象 triple の入る位置）

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- .github/workflows/build.yml（workflow_call、6 対象の matrix ビルド、upload-artifact つき）を再利用する
- 現状の確認（2026-10-31）: ci.yml の targets ジョブが既に ./build.yml を tag: dev で呼び、build.yml が各対象のアーカイブを artifact として upload している。つまり PR ごとの artifact 添付は部分的に存在する。本 PBI の残りはレビュアー体験としての位置づけ（artifact の見つけやすさ、PR 番号入りの一括 artifact、PR 説明からの誘導）を discovery で決めること
- ccusage は pkg.pr.new で同種の仕組みを持つ（外部比較として参照）。artifact 添付を既定とし、pkg.pr.new の採用は discovery で判断する
- release の秘密情報には触れない。署名やリリース用 token は preview には使わない（ci.yml の gate が secrets でなく vars を使うのと同じ理由）
- spec 更新: CI 挙動のみの変更は spec 更新不要。reviewer 向け手順（artifact の取り方）を README または docs/pr/ に追記する
- 再利用時に壊さないこと: build.yml の version stamp（Windows の CRLF 考慮込み）と tar.gz 形式は Homebrew formula と checksum の前提になっている

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
