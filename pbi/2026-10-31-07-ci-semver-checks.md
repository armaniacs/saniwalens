# PBI: cargo-semver-checks を CI に追加する

## ユーザーストーリー
開発者・メンテナとして、lib API（store / core）の非互換変更を PR で検出する機能がほしい、なぜなら公開 API の破壊をリリース前に知り、壊れた状態で出荷することを防ぎたいから。

## 優先度
- 順位: 07 / 全28件
- RICEスコア: 6.0（Reach=1 / Impact=2 / Confidence=3 / Effort=1日）
- 根拠: リスク軽減を目的とする開発者向けの保険で、確信度は高いが外部消費者が限定的なので Reach は低め。他 PBI への依存は無く、08（依存更新）・11（commit lint）と独立に着手できる。

## BDD受け入れシナリオ
Scenario: 非互換な API 変更を含む PR は CI が失敗する
  Given lib がライブラリとして安定した公開 API を提供している
  When  公開 API を壊す（非互換な）変更を含む PR を出す
  Then  CI が失敗し、どの公開 API が非互換になったかが CI の指摘から読める

Scenario: 互換変更のみの PR は green になる
  Given 同じリポジトリで公開 API への非互換変更が無い
  When  互換性を保った変更（API の追加や内部変更）のみを含む PR を出す
  Then  CI は green になり、非互換の指摘は何も出ない

## 受け入れ基準
- [ ] 非互換な公開 API 変更を含むブランチで CI が red になる（検証は合成の非互換変更で行い、検証後に変更は戻してマージしない）
- [ ] 互換変更のみのブランチでは CI が green になる
- [ ] 指摘は CI ログで読める形で出る（どの API が壊れたか、どう壊れたか）
- [ ] 既存の CI ジョブ（lint + format / test）の内容と結果は変わらない
- [ ] 検証コマンド（cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test）は従来どおり green
- [ ] secret（トークン等）が無い環境でも CI が green を出せる（トークン不要のジョブにする）

## テスト戦略
- E2E: 非互換変更を含む実 PR で CI が red、互換変更のみの PR で green になることを実際に確認する
- 統合: 既存 CI ワークフローへのジョブ追加が他のジョブを壊さないこと、ワークフロー構文の検査
- 単体: 検証対象の指定（どの crate を対象にするか、比較基準となるバージョンの扱い）と失敗条件の確認。lib の公開モジュール一覧（adapter / cli / core / session / store）が検査対象に含まれること

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状: lib.rs が公開 API を持つ（src/lib.rs の公開モジュール: adapter / cli / core / session / store）。現状は crates.io に CLI パッケージとして公開している（Cargo.toml、release ワークフローが cargo publish まで実行）。CLI 単体の利用者には破壊の影響が薄いが、ライブラリとして参照される場合の保険になる
- 制約: 検証は CI 上で完結させ、ローカルの開発フロー（just check / just test）は変えない。トークン等の secret に依存しないジョブにする
- spec 更新: 不要（CI 設定の追加であり動作変更でない）。docs/specs/*.md は変更しない
- 関連: .github/workflows/ci.yml、.github/workflows/build.yml、src/lib.rs、Cargo.toml
- 落とし穴: CLI パッケージ部分と lib 部分の区別。比較基準（前回リリースとの比較）の取り方が曖昧だと、常時失敗または常時 green の無意味なジョブになる。初回は基準を最小に固定し、指摘が読めることを最優先にする

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
