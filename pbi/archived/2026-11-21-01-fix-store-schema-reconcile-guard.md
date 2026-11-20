# PBI: store schema 収束対象の未分類表を移行時に失敗させる

## 優先度
- 順位: 01 / 全14件
- RICEスコア: 24.0（Reach=4 / Impact=3 / Confidence=100% / Effort=0.5日）
- 根拠: migrate の契約は「SCHEMA に列を足すだけで既存ストアが収束」
  （src/store.rs:2206-2209）だが、収束ループは手綴りの MIGRATED_TABLES /
  REBUILT_TABLES を走る。subagent_runs の DDL（src/store.rs:60-76）が両リスト外で、
  この表への列追加は既存ストアで決して reconcile されない。次に列を足すと
  既存ユーザーで INSERT が全失敗する。

## ユーザーストーリー
既存ストアを使い続ける saniwalens ユーザーとして、saniwalens を更新しても
分析履歴の入ったストアが壊れずにアップグレードされる状態がほしい。
なぜなら、収束対象から漏れた表への列追加は、既存ユーザーの環境で
INSERT を全件失敗させるからだ。

## 背景
- migrate は doc comment で「declared schema が single source of truth であり、
  SCHEMA に列を足すだけで将来の変更は完結する」という契約を掲げている
  （src/store.rs:2206-2209）
- 実際の収束はリスト駆動で、REBUILT_TABLES の drop/recreate と
  MIGRATED_TABLES の reconcile_columns だけが走る（src/store.rs:2234-2243）。
  リストに載っていない表の drift は誰にも検出されない
- MIGRATED_TABLES は sessions / events / ingested_files / meta の 4表
  （src/store.rs:300）、REBUILT_TABLES は surfaces のみ（src/store.rs:304）。
  SCHEMA 内の subagent_runs（src/store.rs:60-76）はどちらにも分類されていない
- 既存テスト a_store_without_subagent_runs_gains_the_table_and_is_re_extracted
  （src/store.rs:3200-3226）は「表が無い → 作られる」だけを検証していて、
  「表はあるが列が足りない」drift は無防備
- 修正方向: desired 接続の sqlite_master から SCHEMA 内の全表名を列挙し、
  MIGRATED ∪ REBUILT に分類されていない表は移行時に失敗させるガードを入れる。
  subagent_runs は実行履歴を保持する表（src/store.rs:56-59）なので、
  行を捨てない MIGRATED 側に分類する。2つのリストを 1 つの
  TABLE_DISPOSITION 定数に統一してもよい。SCHEMA_VERSION は不変
  （列追加は additive drift で、bump の対象外）

## BDD受け入れシナリオ

```gherkin
Scenario: 収束対象に分類されていない表は移行時に失敗する
  Given 既存ストアがある
  And   SCHEMA 内の表のうち subagent_runs が MIGRATED ∪ REBUILT の
        どちらにも分類されていない
  When  migrate を実行する
  Then  移行はエラーで失敗する
  And   エラーメッセージが subagent_runs が収束対象に分類されていないことを示す

Scenario: subagent_runs への列追加 drift は黙って通らない
  Given subagent_runs に仮想の列を足した desired schema がある
  And   旧形の subagent_runs と既存行を持つ既存ストアがある
  When  migrate を実行する
  Then  drift は検出され、移行は黙って成功しない

Scenario: 分類後の subagent_runs は列追加で既存ストアに収束する
  Given MIGRATED に分類された subagent_runs と、旧形の subagent_runs と
        既存行を持つストアがある
  When  migrate を実行する
  Then  subagent_runs が SCHEMA 宣言どおりの列を持つ
  And   既存の行は失われていない

Scenario: 分類済みの表の既存の収束挙動は変わらない
  Given 旧形の surfaces と旧形の events を持つ既存ストアがある
  When  migrate を実行する
  Then  surfaces は drop/recreate され、events は列追加で収束する
  And   meta の analyzed_at は surfaces が変わったときだけクリアされる
```

## 受け入れ基準
- [x] migrate が、desired 接続の sqlite_master から SCHEMA 内の全表名を列挙し、
  MIGRATED ∪ REBUILT に分類されていない表があればエラーで失敗する
- [x] subagent_runs が収束対象として分類される（MIGRATED 側への追加、または
  TABLE_DISPOSITION 定数への統一のいずれかの形で）
- [x] subagent_runs に仮想列を足した desired schema で migrate が失敗/検出される
  列追加ドリフトの再現テストがある（Red/Green の判定線）
- [x] 旧形 subagent_runs の既存行が、収束後も失われないことを確認するテストがある
- [x] SCHEMA_VERSION は不変（v1 のまま。列追加は migrate が収束する
  additive drift の範囲）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test が green

## テスト戦略
- 単体（判定線）: subagent_runs に仮想列を足した desired schema を用意し、
  migrate が失敗/検出することを assert するテスト1本。現状のコードでは
  drift が黙って通るため Red、修正後に Green になる
- 単体: ガードの境界 — SCHEMA 内の全表が MIGRATED ∪ REBUILT に分類されて
  いれば migrate は成功し、1つでも漏れがあればエラーメッセージが表名を指す
- 単体: 収束の行保持 — 旧形 subagent_runs の既存行が列追加後も残る
  （MIGRATED の「列を足すだけで rebuild しない」意味論。src/store.rs:297-299）
- 回帰: 既存の migrate テスト群（surfaces 再構築: src/store.rs:3170-3197、
  表の新規作成: src/store.rs:3200-3226、fingerprint 保持: src/store.rs:3228 以降）
  が壊れないこと。cargo test 全体 green を確認する

## 見積もり
0.5日（RICE の Effort と同一根拠）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: migrate の doc comment
  （src/store.rs:2206-2209）と実装の乖離が解消されていることを含む）
- [x] ドキュメント更新済み（docs/specs/storage.md の収束契約の記述と
  実装が一致していること）
