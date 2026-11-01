# PBI: テストの scratch dir を RAII guard に閉じ stale 再利用とリークを止める

## ユーザーストーリー
コントリビュータとして、テストの一時ディレクトリがテストの成否にかかわらず必ず消え、
前回の残骸を再利用しないでほしい、なぜなら現行は成功時にも shadow で 1 個残り、
削除失敗を握り潰すと前回の `store.db` のままテストが走るからだ。

## 優先度
- 順位: 8 / 全候補数 10
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.0日）
- 根拠: 候補の「失敗時のみ残る」は反証され、成功時にも毎回 1 個残ることが実測された。
  stale DB の再利用はテストの再現性を壊す。④⑤⑩ の後に置き、同じテストファイルの
  変更を直列化する。

## ビジネス価値
- scratch dir の寿命が型で保証され、panic・早期 return・shadow でも削除される。
- 前回の残骸を黙って再利用せず、衝突は即座に検出される。
- 測定方法: `TMPDIR` の `saniwalens-*` がテスト後に増えないこと。panic 経路でも
  消えること。stale dir に `STALE ROWS` を置いても新しい store が使われること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 13系統）
- `tests/trend_markdown.rs:172` と `:184` が 2 つの dir を作り `let dir` を shadow し、
  `:195` は 2 個目しか削除しない。実測で `*-reason-two-sided` が 64 個、
  `TMPDIR` 全体で 84 dirs / 28.8MB 残っていた。
- `tests/trend_json_golden.rs:122-130` と `src/cli/window.rs:573-578` の helper は
  `remove_dir_all` の失敗を `let _ =` で握り潰し、`create_dir_all` が既存を受理する。
  削除失敗時は前回の `store.db` のままテストが走る（probe P4b で "STALE ROWS" を確認）。
- 根本原因: テスト所有の一時ディレクトリに寿命を表す型が無く、後始末が制御フローの
  末尾文に依存している（RAII 不在）。加えて解放失敗を無視する。

## BDD受け入れシナリオ

```gherkin
Scenario: 成功時も panic 時も scratch dir は消える
  Given ScratchDir を生成した
  When スコープを正常終了する、または panic する
  Then ディレクトリは存在しない

Scenario: 前回の残骸を再利用しない
  Given 同じ名前の古いディレクトリと STALE ROWS 入りの store.db がある
  When ScratchDir を生成する
  Then 古いディレクトリは削除され、新しい空のディレクトリになる

Scenario: shadow で 1 個残らない
  Given trend_markdown の 2 つの fixture が 2 つの scratch dir を使う
  When テストを実行する
  Then 両方のディレクトリが削除される
```

## 受け入れ基準
- [ ] `tests/common/mod.rs` に `ScratchDir` guard を追加する
  （`new` は既存を削除し、NotFound のみ許容 -> `create_dir` で作成 ->
  `Drop` で削除、失敗は stderr に 1 行。`SANIWALENS_KEEP_SCRATCH=1` で保持）
- [ ] `src/scratch.rs` に同じ契約の `#[cfg(test)]` guard を追加し、`src/lib.rs` から
  `#[cfg(test)] mod scratch;` で読み込む（統合テストからは見えないため 2 コピー。
  相互参照コメントを必須にする）
- [ ] `tests/trend_json_golden.rs`、`tests/trend_markdown.rs`、`tests/period_errors.rs`
  の helper を `tests/common` の guard に置き換える。末尾の `remove_dir_all` を削除する
- [ ] `tests/trend_markdown.rs` の shadow を解消し、2 つの dir をそれぞれ guard で
  保持する（`reason-two-sided` / `reason-one-sided`）
- [ ] `src/cli/window.rs` の test module の `example_dir` を `crate::scratch::ScratchDir`
  に置き換え、guard を `opened` より先に宣言する（Drop 順で store が先に閉じる）
- [ ] panic 経路の契約テストを `catch_unwind` で追加する（guard 生成 -> panic ->
  dir が存在しない）
- [ ] stale dir の再利用テストを追加する（古い dir + STALE ROWS -> 新しい空 dir）
- [ ] `.claude/rules/tdd.md` に Scratch dirs 節を追加する（4 文: 合成のみ、
  temp に置く、guard を使う、リポジトリに置かない）
- [ ] `tempfile` などの dev-dependency は追加しない。inline の 25 箇所は本 PBI の
  範囲外とし、台帳に follow-up として記録する
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `cargo test` 前後で `TMPDIR` の `saniwalens-*` が増えないこと（手動確認 + 契約テスト）

### 統合テスト
- trend_json_golden / trend_markdown / period_errors の既存テストが guard で green

### 単体テスト
- 正常終了で消える
- `catch_unwind` の panic で消える
- stale dir を削除して新しい dir を作る
- `SAVEWALENS_KEEP_SCRATCH=1` は実装のみ（環境変数は並列テストで共有されるため
  テストしない）

## 実装アプローチ
- **Outside-In**: shadow リークと stale 再利用の再現テスト -> guard のパイロット
  （trend_json_golden 1 本）-> tests/ 展開 -> window.rs -> tdd.md。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
1 SP（E=1.0日。RICE 12.0）

## 技術的考慮事項
- 依存関係: ④⑤⑩ -> ⑪（`tests/trend_json_golden.rs` と `src/cli/window.rs`）
- `create_dir_all` ではなく `create_dir` を使う。既存を黙って受理しない
- Drop 内で panic しない。削除失敗は stderr の 1 行に留める
- Windows の open handle は未実証。CI は ubuntu のみなので、契約テストは
  「削除を試み、失敗を報告する」までを pin する
- 古い 84 dirs は証拠として残っている。本 PBI では削除しない（手動 cleanup は別）

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `tests/trend_json_golden.rs:122-130` の `scratch_dir`、:251-258 の使用と削除。
- `tests/trend_markdown.rs:172` / `:184` の shadow、:195 の削除。
- `tests/period_errors.rs` の scratch helper。
- `src/cli/window.rs:573-578` の `example_dir`、:663 / :725 の削除。
- `Cargo.toml` に `[dev-dependencies]` は無い。

### 実装手順
1. Red: shadow リークと stale 再利用の再現テスト
2. `tests/common/mod.rs` の guard を作り、golden 1 本でパイロット
3. panic 経路の契約テスト
4. trend_markdown / period_errors へ展開
5. `src/scratch.rs` と window.rs へ展開
6. tdd.md に 1 節
7. 検証（対象テスト -> fmt/clippy -> フルスイート、TMPDIR の増加確認）

### 落とし穴
- `let _ = remove_dir_all` を新しい guard に持ち込まない
- guard を `opened` より後に宣言すると、store の Drop より先に dir が消える
- `SAVEWALENS_KEEP_SCRATCH` のテストで環境変数を並列に触らない
- `tests/common/mod.rs` はテストバイナリではない。各テストで `mod common;` する
- `src/scratch.rs` は `#[cfg(test)]` のみ。統合テストからは見えない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 成功時・panic 時・shadow 時に scratch dir が残らない
- [ ] stale dir を再利用しない
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] tdd.md 更新済み
