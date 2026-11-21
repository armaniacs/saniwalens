# PBI: ストア移行の version guard をトランザクション内に置く

## ユーザーストーリー
旧版と新版の saniwalens を並行して起動するユーザー（プロジェクトごとに
異なるバージョンが使われる場合など）として、version チェックと移行の
書き込みが競合してもストアが壊れない状態がほしい。なぜなら、guard の
読み取りがトランザクション外にあると、判定の後に別バージョンが
user_version を進めても旧判定のまま DDL が走り、新版が正しく読める
ストアを壊す構造的な隙（TOCTOU）があるからだ。

## ビジネス価値
- 優先度: 12 / 13 件中 — SCHEMA_VERSION が未 bump（v1、src/store.rs:295）の
  ため異版混在の並行起動は現状発生不能で、防ぐべき事故がまだ起き得ない。
  同一バージョンの並行では migrate が冪等で、BUSY も可視失敗
  （リトライで回復）にとどまる。本 PBI は防御的強化である
- それでも今やる理由: 最初の SCHEMA_VERSION bump と同時にこの TOCTOU は
  実害に変わる。「新しい版のストアを壊さず拒否する」という guard の意図
  （src/store.rs:2157-2159 のコメント）を構造で保証し、並行 open の
  "database is locked" 即落ちを待機か明確なエラーに変えるコストは小さい
- 根拠: migrate（src/store.rs:2154-2203）は version を autocommit の単発
  読みで取って判定し（:2155-2164）、その後に `conn.transaction()` で
  DDL/reconcile を行い（:2176-2198）、最後に user_version を書き込む
  （:2200）。チェックと書き込みの間にトランザクション境界がなく、guard
  判定と user_version 書込が直列化されていない。加えて busy_timeout は
  src 全域に未設定（ReadWrite open: :635 / ReadOnly: :641-645）で、
  ReadWrite open の移行 tx と並行 open が SQLITE_BUSY で即落ちする

## BDD受け入れシナリオ

```gherkin
Scenario: 新しい版の saniwalens が書いたストアへの移行はトランザクション内でも拒否される
  Given user_version が SCHEMA_VERSION より大きい fixture ストアがある
  When  migrate を実行する
  Then  移行はエラーで失敗する
  And   エラーメッセージが新版へのアップグレードを促し、削除を勧めない

Scenario: 並行する書込接続があるとき移行は DDL を実行せず待機または明確なエラーになる
  Given 別の接続がストアの書込ロックを保持している
  When  migrate を実行する
  Then  DDL は一切実行されない
  And   ロック解放後の待機で成功するか、明確なエラーメッセージで失敗する

Scenario: guard がトランザクション内に移っても既存ストアの移行は変わらない
  Given 旧形の表と既存行を持つ legacy ストアがある
  When  migrate を実行する
  Then  user_version は SCHEMA_VERSION にスタンプされる
  And   既存の行は失われていない
```

## 受け入れ基準
- [x] migrate の version 読み取りと guard 判定が、BEGIN IMMEDIATE の
  トランザクション内・DDL より前に置かれ、user_version 書込と同一 tx で
  直列化される
- [x] user_version を直接設定した fixture ストアで、tx 内の guard が確実に
  bail する異版混在シミュレートテストがある
- [x] busy_timeout の設定（例: 5 秒 — `Connection::busy_timeout`）または
  明確なエラー文面のいずれかが実装されている（選択は実装時決定、文面は
  "database is locked" からの改善を最低限とする）
- [x] guard のエラー文面契約が不変 — "upgrade" を含み "delete" を勧めない
  （src/store.rs:2950-2963 のテストが修正後もパスし続ける）
- [x] 既存の migrate テスト群がバイト同一である
- [x] cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体（判定線）: 異版混在シミュレート — `PRAGMA user_version` を直接
  SCHEMA_VERSION + 1 に設定した fixture ストアで migrate が bail することを
  assert するテスト 1 本。guard を tx 内へ移した後も機能が保たれることを
  固定する
- 単体: 直列化の観測 — 別接続が書込ロックを保持している状態で migrate を
  呼ぶと、DDL が一切実行されずに選択した挙動（待機後成功 / 明確なエラー）
  に従うこと。BEGIN IMMEDIATE 化により guard 読み取り自体がロック解放を
  待つ位置に変わる。実装で「明確なエラー」を選んだ場合はその文面を
  assert する
- 単体: tx 内の bail の後始末 — guard で bail した接続をそのまま使い、
  ストアの表構造と user_version が変化していないこと（tx drop =
  ロールバック）を assert する
- 回帰: 既存の migrate テスト群（version スタンプ: src/store.rs:2939-2948、
  新版拒否: :2950-2963、subagent_runs の表作成: :3170、surfaces 再構築系）
  をバイト同一のまま green に保つ。cargo test 全体 green を確認する

## 実装アプローチ
1. busy_timeout を先に設定する — 単一 open 経路（Store::open ReadWrite →
   Connection::open: src/store.rs:635、from_connection: :751）で接続に
   `busy_timeout` を設定する（例: 5 秒）。待機にするか明確なエラー文面に
   するかは実装時に決定する。ReadOnly 経路（:641-645）は変更不要
2. migrate を BEGIN IMMEDIATE に変更する — `conn.transaction()` を
   `TransactionBehavior::Immediate` に変え、:2155 の version 読み取りと
   :2156-2164 の guard を tx 内の先頭（DDL より前）へ移動する。guard は
   DDL 前なので bail 時の tx drop は書込なしのロールバックで安全
3. desired 接続の構築（in-memory、:2166-2170）はストアのロックに関与しない
   ため tx 外に置いたままにする
4. 異版混在のシミュレートテストを追加する
5. tx の位置に依存するコメント（:2172-2175）を移動に合わせて更新する

## 見積もり
0.5日

## 技術的考慮事項
- 現行（SCHEMA_VERSION = 1、src/store.rs:295）では異版混在の並行起動は
  発生不能 — 本 PBI の挙動の可視変化は並行時のみで、ロールバック不要
- BEGIN IMMEDIATE は他接続の書込ロックを待つ — busy_timeout を先に設定
  しないと guard の tx 自体が BUSY で落ちる（順序が重要）
- 直列化が防ぐのは「判定後に user_version が進み、旧判定のままこのビルドの
  形を上書きする」方向の事故。逆方向（新版が書いたストアを旧版が開く）は
  guard の bail 文面（"upgrade saniwalens to read it"）が担う
- migrate の doc comment（src/store.rs:2138-2153）と tx コメント
  （:2172-2175）は実装の一次資料 — 構造変更に同期させて更新する

## 実装者向け注記

### 現状コードの確認
- migrate 本体: src/store.rs:2154-2203。version 読み取り :2155（autocommit
  の単発読み）→ guard :2156-2164（version > SCHEMA_VERSION で bail、文面
  "the store was written by a newer saniwalens ... upgrade saniwalens to
  read it"）→ `conn.transaction()` :2176（遅延 BEGIN）→ DDL/reconcile
  :2178-2198 → user_version 書込 :2200 → commit :2201
- 単一 open 経路: Store::open（src/store.rs:626-649）→ ReadWrite は
  Connection::open :635、ReadOnly は open_with_flags :641-645 →
  ReadWrite 側は from_connection :751 が migrate まで行う
- busy_timeout の設定は src 全域に無い（grep で 0 件）
- SCHEMA_VERSION = 1（:295）。MIGRATED_TABLES :300 / REBUILT_TABLES :310
- 既存テスト: opening_stamps_the_current_schema_version :2939-2948 /
  a_store_written_by_a_newer_saniwalens_is_refused_without_advising_deletion
  :2950-2963 / a_store_without_subagent_runs_gains_the_table_and_is_re_extracted
  :3170

### 実装手順
1. busy_timeout を設定する（例: 5 秒）。待機か明確なエラー文面かを実装時
   決定し、選択した根拠をテストまたはコメントに残す
2. migrate を `TransactionBehavior::Immediate` に変更し、version 読み取り +
   guard を tx 内の先頭へ移動する
3. :2172-2175 の tx コメントを移動先に合わせて更新する
4. 異版混在のシミュレートテストを追加する
5. cargo fmt --check / cargo clippy --all-targets --all-features --
   -D warnings / cargo test で green を確認する

### 落とし穴
- BEGIN IMMEDIATE は他接続の書込ロックを待つ — busy_timeout 未設定のまま
  だと guard の tx 自体が BUSY で落ちる（順序に注意: busy_timeout を先に
  設定）
- migrate の既存テスト（store.rs の migrate 系、subagent_runs 列追加
  ドリフト等、:2900-2963 / :3170）がバイト同一であること — 本修正は構造の
  直列化であって単独起動時の挙動変化ではない
- guard は tx の最初（DDL より前）に置く — 後ろに置くと bail 時に無駄な
  DDL が走り、tx drop のロールバックに頼る形になる。明示 rollback は不要
  （tx drop でロールバック）だが位置の契約は保つ
- TOCTOU の真の再現テストはインターリーブの強制が困難 — 境界の固定
  （fixture への user_version 直設定 + ロック保持下の挙動）で代替する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] version 読み取り・guard 判定・user_version 書込が同一の
      BEGIN IMMEDIATE tx 内にある
- [x] busy_timeout または明確なエラー文面が実装済み（選択と理由が
      テスト/コメントで追跡可能）
- [x] 既存 migrate テスト群がバイト同一で green
- [x] cargo fmt --check / cargo clippy --all-targets --all-features --
      -D warnings / cargo test が green
- [x] コードレビュー完了（migrate の doc comment :2138-2153 と tx 構造の
      記述が実装と一致していることを含む）
- [x] docs/specs/storage.md に並行起動時の guard 契約の記述があれば
      実装と一致していること
