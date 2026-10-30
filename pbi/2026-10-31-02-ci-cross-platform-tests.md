# PBI: CI matrix で Windows / macOS も test を実行する

## ユーザーストーリー
saniwalens の開発者・メンテナとして、check と test を複数 OS で実行する CI がほしい。
なぜなら OS 差分（パス区切り、CRLF、C コンパイラ、ファイル権限）をリリース後ではなく
PR の時点で検出し、リリース対象の全環境で品質を保てるからだ。

## 優先度
- 順位: 02 / 全28件
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1日）
- 根拠: リスク軽減効果が大きく工数1日。01 と同点。順位は 01 の根拠のとおり
  supply chain の防御を先にする判断のため 2 番目。

## BDD受け入れシナリオ
Scenario: PR で 3 OS の test が走り、全 green なら merge 可能
  Given 開発者が PR を出した
  When  CI が実行される
  Then  Linux / macOS / Windows の 3 環境で check と test が走る
  And   3 環境すべて green のときだけ PR が merge 可能になる

Scenario: Windows のみ失敗したら PR が red になる
  Given PR の変更が Windows 固有の挙動（パス区切りや CRLF）で壊れている
  When  CI が実行される
  Then  Windows の check か test が red になり、PR は merge できない状態になる
  And   失敗した OS が PR の status check で確認できる

## 受け入れ基準
- [ ] check（fmt + clippy）と test が Linux / macOS / Windows の 3 環境で実行される
- [ ] 1 OS の失敗で他 OS の結果が消えない（fail-fast を off にする）
- [ ] macOS / Linux は現行の nix 経路（cachix/install-nix-action と
  nix develop -c just ...）を踏襲する
- [ ] Windows は dtolnay/rust-toolchain 経路で実行する（nix は Windows を対象外のため）
- [ ] Windows runner でも bundled SQLite（rusqlite bundled）のビルドに必要な
  C コンパイラが揃い、test が green になる
- [ ] 既存ジョブ（build / targets / package）の構成を壊さない（check / test の matrix 化のみ）
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: PR を出して 3 環境の status check が揃うこと（Actions の実行結果）。
  Windows 固有のパス処理を壊した確認 PR で red になること
- 統合: Windows runner 上で cargo test（ストア生成やファイル権限まわりを含む）が通ること
- 単体: 既存の単体テストが OS 差分（区切り文字、CRLF、HOME と USERPROFILE の差）を
  吸収して通ること

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状 .github/workflows/ci.yml の check / test は ubuntu-latest のみ。build.yml は
  matrix ビルドのみ（Linux 2 arch / macOS 2 arch / Windows 2 arch）で、check / test は無い
- nix は Windows を対象外（flake の各 system は macOS / Linux のみ）。Windows は
  package ジョブと同じ dtolnay/rust-toolchain + Swatinem/rust-cache 経路にする。
  pinned toolchain（rust-toolchain.toml）との整合に注意
- rusqlite は bundled feature で SQLite を C からビルドするので、どの OS でも
  C コンパイラが必要。macOS / Linux は nix stdenv、Windows は runner に MSVC の
  C toolchain が揃っている（build.yml のコメントが実例）
- check / test の matrix は各 OS で 1 arch ずつの native 実行で十分。
  リリース対象の全 arch は既存 targets ジョブが担保する
- Windows runner の checkout が CRLF になる点（build.yml の lock 書き換えが
  CRLF を許すパターンを使う教訓）と、step の shell が Git Bash になる点
  （build.yml の defaults）を踏襲する
- spec 変更は不要（CI のみの変更）
- エビデンス: .github/workflows/ci.yml、.github/workflows/build.yml
  （Windows を含む matrix、CRLF と shell の教訓）、.gitattributes
- 関連コード: .github/workflows/ci.yml、justfile（check / test の定義）、rust-toolchain.toml

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
