# PBI: coverage レポートを CI に追加する

## ユーザーストーリー
saniwalens の開発者・メンテナとして、cargo-llvm-cov で coverage を計測し PR に
サマリーを出す仕組みがほしい。なぜなら TDD 主義のプロジェクトで、テストが
追いついていない経路（adapter のハーネス対応、core の境界）を見える化し、
テストの抜けを PR の時点で把握できるからだ。

## 優先度
- 順位: 03 / 全28件
- RICEスコア: 8.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.5日）
- 根拠: テスト文化の強化で工数小。ただしゲートではなく可視化が主目的のため
  01 / 02（CI のゲート系）の次。

## BDD受け入れシナリオ
Scenario: PR で coverage サマリーが出る
  Given 開発者が PR を出した
  When  CI が coverage 計測を実行する
  Then  PR に crate 全体の coverage サマリー（行 coverage の率）が出る
  And   変更が入ったファイルの coverage も併せて確認できる

Scenario: トークンが無い環境でも green になる
  Given PR コメントを投稿するトークンが設定されていない
  When  CI が coverage 計測を実行する
  Then  コメント投稿は省略され、coverage 計測ジョブ自体は green で完走する
  And   サマリーは CI の実行結果から確認できる

Scenario: coverage が下がる変更でも red にはならない
  Given PR の変更で行 coverage の率が下がった
  When  CI が coverage 計測を実行する
  Then  サマリーに下がったことが現れるが、ジョブは red にならない
  （coverage は可視化であってゲートではない。ゲート化するかは別の判断として後で決める）

## 受け入れ基準
- [x] CI に coverage 計測のジョブが追加され、cargo-llvm-cov で計測される
- [x] PR に行 coverage の率と内訳（step summary＋artifact。PRコメントは出さない最小構成）（モジュール別か変更ファイル別）のサマリーが出る
  （まずは実行結果への出力の最小構成を入れる）
- [x] coverage の率はゲートにしない（閾値なし）（閾値での失敗はこの変更では入れない）
- [x] PR コメント投稿はトークンが設定されているときだけ行われ、無い環境でも（コメント機能自体なし。summary＋artifactのみでトークン不要）
  ジョブは green で完走する（secret を if 条件にしてジョブ自体が消えない）
- [x] LCOV / html の成果物はジョブの artifact として残す（ダウンロードして詳細を見られる）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: PR で coverage ジョブが完走しサマリーが実行結果に現れること。
  トークン無しの環境（fork からの PR）でも green であること
- 統合: cargo-llvm-cov の計測結果（LCOV）と既存テスト一式（cargo test）の整合。
  nix 経路と dtolnay 経路のどちらでも計測できること
- 単体: coverage 計測自体はプロダクトコードに手を入れないため単体テストの追加は無し。
  既存テスト一式が green であること

## 見積もり
1.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- CI は nix ベース（.github/workflows/ci.yml 参照）。llvm-cov は flake devShell
  （flake.nix）に無いので nixpkgs から導入する（cargo-llvm-cov は cargo の
  サブコマンドとして動く）。nix に入れるとローカルの手順も揃う
- cargo-llvm-cov は test を内部で再実行する。test ジョブとの重複実行になるため、
  test ジョブ内で計測するか別ジョブにするかを決める
- PR コメントの教訓（過去の PR #9 / #10）: secret を if 条件にするとジョブ自体が
  評価されない。変数で条件化するか、token が空なら投稿だけ skip する分岐にする
- coverage の数字そのものに目標を置かない。TDD 主義（.claude/rules/tdd.md）では
  赤緑リファクタの実践が主で、レポートは経路の抜けを見つける道具
- spec 変更は不要（CI のみの変更）
- エビデンス: .github/workflows/ci.yml、flake.nix（devShells.default の packages）、
  justfile、.claude/rules/tdd.md
- 関連コード: .github/workflows/ci.yml、flake.nix、justfile

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（ローカル検証分。CI実行は初回で確認 — 外部。llvm-toolsはCI側で導入）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（CIのみの変更。spec更新不要）
