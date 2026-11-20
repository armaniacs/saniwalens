# PBI: 汎用ユーティリティを契約上の層へ移す

## 優先度
- 順位: 07 / 全14件
- RICEスコア: 5.0（Reach=5 / Impact=1 / Confidence=100% / Effort=1日）
- 根拠: 新規コード（handoff, guide, doctor）を書く過程で、汎用ヘルパーが
  レイヤー契約と無関係なモジュールに置かれ、他レイヤーからの import が
  発生した。store 層は SQLite の永続化とクエリのみを持つべき
  （docs/specs/architecture.md の層の表）だが、汎用 fs 権限ヘルパーを
  export して cli が消費している。adapter 層は両入力フォーマットの知識
  のみを持つべきだが、プロセス env 全般のユーティリティがハーネス継ぎ目
  モジュールに住み、shell と view が import している

## ユーザーストーリー
メンテナとして、汎用の env・fs ヘルパーが各層の契約どおりの場所に住み、
層をまたぐ import が契約に沿った状態がほしい。なぜなら handoff・guide・
doctor の新規コードが既存の置き場所を模倣するたびに契約外の import が
積み重なり、層の境界が時間とともに摩耗するからだ。

## 背景
- store 層が汎用 fs 権限ヘルパーを所有・輸出している:
  src/store.rs:509 `pub(crate) fn create_private_dir_all` — SQLite 層が
  XDG で汎用の 0700 ディレクトリ作成を所有している。これを cli が
  輸入している: src/cli/handoff.rs:24（use）と :146（save_plan 内の
  呼び出し）。同種の 0600 ファイル書き込み `write_private_file` は
  正しく src/cli/deliver.rs:326 にあり、0700 dir 作成もその隣が契約上の居場所
- adapter 継ぎ目にプロセス env ユーティリティが駐留している:
  src/adapter/harness.rs:776 `xdg_from_env`、:790 `home_dir` — どちらも
  ハーネス入力フォーマットの知識ではない。shell と view が輸入して
  いる: src/cli/mod.rs:10（use）と :315、src/cli/views/guide.rs:4 と
  :712（他 :725）、src/cli/views/doctor.rs:7 と :467
- XDG state-base 解決規則の手書き複製: src/cli/handoff.rs:99-112
  （state_saniwalens_dir）が src/session.rs:538-546（store_path 内）の
  「絶対 XDG_STATE_HOME 優先／相対は無視／絶対 home 必須」規則を
  エラー文面まで複製しており、handoff.rs 自身の doc comment（:94-98、
  :100-102）が「session.rs と同じ規則」と同期を自己申告している。
  規則が変わる箇所が二つあり、片方だけ更新されるリスクを実物が示している
- 契約の根拠: docs/specs/architecture.md の層の表 — store は
  "Persisting and querying sessions / surfaces / events in SQLite" のみ、
  adapter は両入力の raw shapes の知識のみ

## BDD受け入れシナリオ

```gherkin
Scenario: handoff の plan 保存は移動後も同じ権限で行われる
  Given create_private_dir_all が src/cli/deliver.rs の write_private_file の隣に移動している
  When handoff が plan を保存する（save_plan）
  Then dir は 0700 で作られ、plan ファイルは 0600 で書かれる（振る舞い不変）
  And handoff.rs の store 層からの import は消えている

Scenario: shell と view の home 解決は移動後も同じ値を返す
  Given home_dir と xdg_from_env が src/session.rs の env 読み取り担当に移動している
  When optimize / guide / doctor がそれぞれ home 解決を必要とする
  Then いずれも移動前と同じ値を得る（HOME 優先・Windows の実在チェックを含む）
  And shell と view の home_dir の import 先は adapter から session に付け替わっている

Scenario: handoff の state-home 解決は store と同一の規則を一つの関数で共有する
  Given session.rs の store_path が使う絶対 state-base 解決が pub(crate) の小関数に切り出されている
  When handoff が state_saniwalens_dir で plans / backups dir を解決する
  Then 絶対 XDG_STATE_HOME 優先／相対は無視／絶対 home 必須の規則が複製ではなく共有される
  And home が無い場合の失敗は with_context で文脈付きのまま失敗する
```

## 受け入れ基準
- [x] home_dir と xdg_from_env が src/session.rs の env 読み取り担当
  （env_roots 側）に移動し、src/adapter/harness.rs から定義が消えている
- [x] shell と view（src/cli/mod.rs、src/cli/views/guide.rs、
  src/cli/views/doctor.rs）の home_dir import が adapter から session へ
  付け替わっている
- [x] create_private_dir_all が src/cli/deliver.rs の write_private_file の
  隣に移動し、src/store.rs は private な ensure_owner_only（:536 付近）
  だけを持ち、pub(crate) 輸出が消えている
- [x] src/session.rs の store_path が使う絶対 state-base 解決が
  pub(crate) の小関数（例: state_base）に切り出され、handoff.rs の
  state_saniwalens_dir から呼ばれている（エラー文脈は with_context で足す）
- [x] 振る舞い不変: 既存テストが無修正または import 付け替えのみで green
  （store_path の XDG 規則テスト、handoff の純関数テスト、
  tests/xdg_shadow_parity.rs が証人）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features --
  -D warnings、cargo test が green

## テスト戦略
- 単体: 振る舞いを固定する既存テストがそのまま証人になる。移動対象に
  付随するテスト（deliver.rs の write_private_file 系、session.rs の
  store_path 規則テスト :1503 付近の一連、handoff.rs の
  state_saniwalens_dir 系）は移動元→移動先へ assert を変えずに付け替える。
  新規テストは原則不要
- 統合: tests/xdg_shadow_parity.rs が XDG 解決の振る舞い不変を監視する
- E2E: cargo test 全体が green。振る舞いは変わらないため golden 差分は無い

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- **着手順序（必須）**: 本 PBI は src/adapter/harness.rs と src/session.rs に
  触れる。同日の PBI 04
  （2026-11-21-04-refactor-advisor-language-vocabulary.md、session.rs）が
  先行バッチ、PBI 09
  （2026-11-21-09-refactor-harness-facts-assembly.md、adapter/harness.rs）が
  後続バッチ。**PBI 04 の完了後に着手し、PBI 09 は本 PBI の完了後に
  着手すること**（バックログの依存: 04 → 07 → 09）
- どれも定義移動 + import 修正のみで、ロジックの変更は含まない
- 移動 1（env ユーティリティ → session）: 定義は src/adapter/harness.rs:776
  （xdg_from_env）と :790（home_dir）。adapter 内の消費者（harness.rs:260、
  config.rs:11/:734、kilocode.rs:19-20/:702/:869-870、opencode.rs:15-16/
  :710-719）も import 付け替えの対象。home_dir の Windows 実在チェックの
  理由を書いた doc comment（:783-789 付近）は移動先でも保持する
- 移動 2（fs 権限ヘルパー → cli/deliver）: 定義は src/store.rs:509。
  store 内部の呼び出し（Store::open の :652 付近）は store の private な
  実装に置き換え、pub(crate) 輸出をやめる。移動先では deliver.rs:326 の
  write_private_file と doc comment の語彙を揃える
- 移動 3（state-base 解決の共有化）: src/session.rs:538-545 の match を
  pub(crate) の小関数（例: state_base(state_home, home)）に切り出し、
  store_path（:525）から呼ぶ。handoff.rs:99-112 は複製をやめて呼び先を
  変える。エラー文面「HOME is not set to an absolute path」は
  session.rs 側に残り、handoff 側は with_context で呼び出し文脈を足す。
  handoff.rs の doc comment（:94-98、:100-102）の自己申告は複製の証拠で
  あり、共有化後に削れる
- 契約の根拠: docs/specs/architecture.md の層の表。振る舞いは変わらない
  ため spec 同期の該当は無いはずだが、層の記述に触れる場合は
  .claude/rules/spec-sync.md に従う

## Definition of Done
- [x] 全BDDシナリオが検証されパスする（振る舞い不変のため既存テストが
  証人。証人となったテストを記録する）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・
  spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
