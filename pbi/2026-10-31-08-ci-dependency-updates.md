# PBI: dependabot / renovate で依存更新を自動化する

## ユーザーストーリー
開発者・メンテナとして、Cargo 依存の更新 PR を自動で開く仕組みがほしい、なぜなら依存が古くなるのを防ぎ、更新の手間を減らしたいから。

## 優先度
- 順位: 08 / 全28件
- RICEスコア: 6.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: 保守の自動化で工数が小さく確信度は高いが、利用者に直接見える価値は中程度。07（cargo-semver-checks）と独立だが、07 を先に入れると更新 PR の互換性確認も同時に効く。

## BDD受け入れシナリオ
Scenario: 更新のある依存に対して自動 PR が開かれる
  Given 依存関係に新しいバージョンが存在する
  When  自動更新の定期的な実行時刻を迎える
  Then  更新可能な依存について更新 PR が自動で開かれる

Scenario: 更新 PR で CI が走り、落ちた場合は merge されない
  Given 自動更新の PR が開かれた
  When  その PR の CI を実行する
  Then  CI が green の場合のみ merge できる。CI が落ちた PR は merge されず、原因が分かる状態で残る

## 受け入れ基準
- [ ] 自動更新の設定が新規ファイルとして追加される（.github/dependabot.yml 新設を想定、cargo ecosystem）
- [ ] 更新のある依存に対して自動 PR が開かれる（実確認は導入後 1 回目の実行で行う）
- [ ] 更新 PR で既存の CI（lint + format / test）が走る
- [ ] 自動更新が開く PR でも、secret（トークン等）が無い環境で CI が green になる（#9/#10 の教訓。現行 CI の CACHIX ゲートは vars ベースの条件化で、secret が無くても workflow が評価されて green になる: .github/workflows/ci.yml）
- [ ] 依存のピン留め意図（Cargo.toml のコメントにある理由）を持つ依存について、更新対象から除外するか、PR を見て閉じる運用かを設定のコメントで明示する
- [ ] 既存ワークフローの挙動は変わらない

## テスト戦略
- E2E: 導入後、更新 PR が実際に開かれること、その PR で CI が green になることを確認する
- 統合: 設定ファイルの構文検査と、既存 CI ワークフローとの組み合わせ（更新 PR で CI が走ること）の確認
- 単体: 除外対象（ピン留め依存）の指定と、実行頻度・対象ディレクトリの設定値の確認

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 新設: .github/dependabot.yml（cargo ecosystem、対象はルートの Cargo.toml）。renovate にする場合は同内容を .github/renovate.json 等に置く。どちらか一方に絞る（重複を許さない）
- 制約: トークンが無くても CI が green になる条件化（#9/#10 の教訓）。dependabot の PR には secret が渡らない前提で、既存 CI ジョブがそのまま green になることを確認する
- ピン留め依存の扱い: rusqlite は 0.32 にピン留め（新しめの libsqlite3-sys が stable Rust でビルドできない。Cargo.toml のコメントに記録済み）。dependabot がこの依存の更新 PR を出すと CI が落ちるので、ignore 設定で除外するか、PR を見て閉じる運用を設定のコメントに残す
- spec 更新: 不要
- 関連: .github/dependabot.yml（新設）、.github/workflows/ci.yml、Cargo.toml / Cargo.lock

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
