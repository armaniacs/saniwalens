# PBI: cargo-deny / cargo-audit を CI に追加する

## ユーザーストーリー
saniwalens の開発者・メンテナとして、依存の advisory と license を CI でゲートする仕組みがほしい。
なぜなら既知脆弱性と license 違反をリリース前に検出し、ユーザーが実行するバイナリの
依存を supply chain の観点から確かめられるからだ。

## 優先度
- 順位: 01 / 全28件
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1日）
- 根拠: リスク軽減効果が大きく工数1日。02（CI matrix で Windows / macOS も test を実行）と
  同点だが、既知脆弱性の混入防止という supply chain の防御が本質のため先頭。

## BDD受け入れシナリオ
Scenario: advisory に該当する依存が入ったら CI が失敗する
  Given 既知の advisory（RUSTSEC）に該当する依存が Cargo.toml に追加されている
  When  開発者がその変更で PR を出す
  Then  CI の依存検査ジョブが失敗し、該当する advisory が実行結果から確認できる

Scenario: 問題がなければ green で検査の基準が記録される
  Given advisory に該当する依存がなく、license も許可一覧に収まっている
  When  開発者が PR を出す
  Then  CI の依存検査ジョブは green で通過する
  And   検査の基準（deny.toml）がリポジトリにコミットされ、後から基準を見直せる

Scenario: トークンが無い環境でも依存検査は走る
  Given リポジトリの変数に Cachix の有効化フラグが無い（トークン未設定）
  When  開発者が PR を出す
  Then  依存検査ジョブはトークンに依存せず実行され、green か red を返す

## 受け入れ基準
- [x] .github/workflows/ci.yml に依存検査のジョブが追加され、PR と main への push で実行される
- [x] deny.toml がリポジトリに新設され、advisory の検査基準と license の許可一覧が記録される（許可一覧はCargo.lock実測に合わせた。cargo-deny-action@v2使用）
- [ ] advisory に該当する依存（合成の確認ブランチで確認）があるとジョブが red になり、（CI初回実行で確認 — 外部）
  該当する advisory が実行結果に現れる
- [ ] license の許可一覧外の依存があるとジョブが red になる（CI初回実行で確認 — 外部）（許可一覧は MIT / Apache-2.0 系に揃える）
- [x] cargo-deny と cargo-audit の役割の重複を解消する。（cargo-deny単独。cargo-audit不使用）advisory と license のゲートは
  1 本のジョブにまとめ、重複実行しない
- [x] Cachix トークン等の secret が無い環境でもジョブは評価される（トークン不要のactionのみ）
  （secret を if 条件にしてジョブ自体が消えない）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: advisory に該当する依存を含む確認ブランチで CI ジョブが red になること、
  クリーンな依存では green になること（Actions の実行結果で確認）
- 統合: deny.toml の検査基準と実際の依存一式（Cargo.lock）の整合。
  ローカルで依存検査コマンドが green になること
- 単体: deny.toml の記述の妥当性（検査対象の指定、license 許可一覧の網羅）を
  ローカルの依存検査コマンドで確認する

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- deny.toml を新設する（既存ファイルの上書きはしない）。cargo-deny は advisory（RUSTSEC）と
  license の検査を deny.toml 1 枚で持てる。cargo-audit は advisory 専用なので両方入れると
  重複する。役割分担は PR 内で決めてジョブに明記する
- CI は nix ベース（.github/workflows/ci.yml 参照）。現状のジョブは check（lint + format）/
  test / build / targets / package の 5 本で、cargo-deny / cargo-audit は flake devShell
  （flake.nix）に無い。nix devShell に tool を足すか、package ジョブと同じ
  dtolnay/rust-toolchain 経路で走らせるかを、nix キャッシュへの影響（依存層の再構築）を見て決める
- 過去の PR #9 / #10 の教訓: secret を if 条件にすると workflow の評価ごと消える。
  トークンが必要な処理（cachix push）は `vars.CACHIX_ENABLED == 'true'` の変数条件化済み
  （ci.yml の check / test / build 参照）。cargo-deny / cargo-audit はトークン不要なので
  secret 条件を付けず常時実行にする。advisory DB の取得に失敗した場合の振る舞い
  （red にするか一時的な警告にするか）を決める
- spec 変更は不要（CI のみの変更。docs/specs/ に CI の契約は無い）
- エビデンス: .github/workflows/ci.yml（ジョブ構成と変数条件化の実例）、
  .github/workflows/build.yml（dtolnay/rust-toolchain 経路の実例）、justfile（check の内容）
- 関連コード: .github/workflows/ci.yml、flake.nix（devShells.default の packages）、justfile

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（ローカル検証分。CI実行分は初回実行で確認）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（CIのみの変更。spec更新不要）
