# PBI: scratch guard の 2 コピーに「削除完了後に claim 解放」の契約 pin を置く

## ユーザーストーリー
保守担当として、`ScratchDir` の 2 コピーが同じ順序契約を持つことをテストで保証して
ほしい、なぜなら「unclaim は remove の後」がコメントだけなので、片方の Drop で順序を
入れ替える変異をどのテストも落とせないからだ。

## 優先度
- 順位: 4 / 全候補数 9
- RICEスコア: 4.0（R=1 / I=2 / C=3 / Effort=1.5日）
- 根拠: 影響はテスト専用コードに閉じるが、2 コピーの片方だけが壊れる形は人の目に
  入りにくい。I=2 は「壊れたときに起きるのはテスト間の削除競合」で、CI が不安定に
  なるため。E=1.5日 は抽出 helper と自己テスト（fixture）を含む。

## ビジネス価値
- 「同じパスの guard が生きている間は削除しない」という順序契約が、2 ファイルの
  コメントではなくテストで保証される。
- 片方だけ編集した PR が赤くなり、2 コピーの同期が強制される。
- 測定方法: 片方の Drop で `unclaim` を `remove_dir_all` の前に移すと
  `cargo test --test scratch_dir_contract` が赤になること。

## 背景（2026-11-02、round4 最終 whole-branch review M6）
- `src/scratch.rs:114-133` と `tests/common/mod.rs:115-134` に同じ `impl Drop for
  ScratchDir` がある。どちらも `keep_scratch()` の早期 return -> `remove_dir_all`
  -> `unclaim` の順で、`:128-130`（tests/common は `:129-131`）に
  「Release the claim only after the removal: a same-path `new` racing this Drop must
  still see the path as live and panic」というコメントがある。
- 既存の pin はあるが順序を見ていない。`tests/scratch_dir_contract.rs` は
  tests/common 側の寿命・panic・stale 再利用・重複拒否・file パスを pin し、
  `src/scratch.rs` の `mod tests` は unit 側を pin する。重複拒否のテストは live な
  guard に対して `new` するだけで、Drop の順序に依存しない。
  `a_dropped_name_can_be_reused` も逐次実行で、順序を問わない。
- したがって「unclaim を remove の前に戻す」変異は両コピーとも緑のまま通る。契約を
  表現しているのはコメントだけ。
- 2 コピーである理由は PBI 50 のとおり（統合テストからは `#[cfg(test)]` の
  `src/scratch.rs` が見えない）。統合できない以上、契約の一致はテストで pin する
  しかない。
- 根本原因: PBI 50 が 2 コピーの契約を「相互参照コメント必須」で済ませ、機械的な
  一致検査を入れなかったこと。

## BDD受け入れシナリオ

```gherkin
Scenario: 2 コピーが同じ Drop 順序を持つ
  Given src/scratch.rs と tests/common/mod.rs の impl Drop for ScratchDir
  When 共有 contract test が両者を読む
  Then どちらも remove_dir_all が unclaim より先で、keep_scratch の早期 return が先頭にある
  And コメントと空行を除いた本体が一致する

Scenario: 片方だけ順序を変えると赤
  Given unit 側の Drop で unclaim を remove の前に移した
  When cargo test --test scratch_dir_contract を回す
  Then 赤になり、message が 2 コピーの同期を名指す

Scenario: 両方を同じく壊しても赤（順序 assert）
  Given 両コピーで unclaim を先に移した
  When 契約テストを回す
  Then 順序 assert が赤になる（本体一致だけでは通ってしまう）
```

## 受け入れ基準
- [ ] `tests/scratch_dir_contract.rs` に `the_two_copies_share_the_drop_contract` を
  追加する。2 ファイルを読み、`impl Drop for ScratchDir` ブロックを抽出し、(a) コメント
  行と空行を除去して前後空白を正規化した本体が一致、(b) 各ブロックで `remove_dir_all`
  の位置 < 最後の `unclaim(` の位置、(c) `keep_scratch` の早期 return が remove より
  前、を assert する
- [ ] 抽出は helper 1 本（`drop_body(path) -> String`）に閉じる。ブロックが見つから
  ない・2 つ以上ある・backtick 不整合などの異常は panic にする（緑にしない）
- [ ] helper の自己テストを 2 本: 片方だけ順序を変えた fixture で「一致しない」こと、
  両方同じく順序を変えた fixture で「順序 assert が落ちる」こと。fixture は
  `tempfile` を足さず、既存の `ScratchDir`（PBI 50）に書く
- [ ] 変異確認を PR 本文に記録: 片方だけ `unclaim` を前に移す -> 赤 / 両方移す -> 赤 /
  `keep_scratch` の早期 return を remove の後に移す -> 赤
- [ ] production code は変えない。test-only hook（Drop 段階を AtomicUsize に記録する
  等）は採らない。production と test で別経路になり、2 コピーの一致を保証しない
- [ ] `.claude/rules/tdd.md` の Scratch dirs 節に「2 コピーは順序契約を共有 test で
  pin する」を 1 句
- [ ] `cargo test --test scratch_dir_contract`、`cargo test --lib`、fmt、clippy が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- なし（テスト専用コードの契約。`cargo test` の一部として回る）

### 統合テスト
- `the_two_copies_share_the_drop_contract`（repo の 2 ファイルを読む）
- 既存の `tests/scratch_dir_contract.rs` の 10 本が green のまま（順序を変えても
  寿命の観測は通るので、既存テストが「順序を見ていない」ことの証拠になる）

### 単体テスト
- `drop_body` の抽出: 正常な 2 ファイル / ブロック欠落で panic / 2 ブロックで panic
- fixture による赤の証明: 片方だけ順序変更（不一致で赤）、両方変更（順序 assert で赤）

## 実装アプローチ
- **Outside-In**: まず「片方だけ順序を変えても緑」を手元で確認（赤くならないことの
  確認）-> 契約テストを書く -> 同じ変異で赤を確認 -> fixture の自己テスト ->
  tdd.md に 1 句。実装は subagent-driven-development で行う。

## 見積もり
1.5 SP（E=1.5日。RICE 4.0）

## 技術的考慮事項
- 依存関係: PBI 50 の着地後（2 コピーが存在することが前提）。他の PBI とはファイルが
  重ならない
- 代替案 C（thread で同じパスの `new` を競わせて実 interleaving を見る）は非決定的で
  flaky。採らない
- テキスト一致だけでは「両方同じく間違える」を許すので、順序 assert を必ず併置する
- パスは `CARGO_MANIFEST_DIR` 起点で読む（`tests/pure_join_placement.rs:78-83` の
  `read` と同じ流儀）。cwd 依存にしない
- 非機能要件: なし

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/scratch.rs:114-133` の Drop（`:128-130` がコメント、`:131` が unclaim）、
  `tests/common/mod.rs:115-134` の Drop（`:129-131` がコメント、`:132` が unclaim）。
- `tests/scratch_dir_contract.rs` の既存 10 本（寿命・panic・stale・重複・file・
  NotFound・report 失敗）。順序を見ていない。
- `src/scratch.rs` の `mod tests`（unit 側の寿命・stale・重複）。
- 既実装の確認: 2 コピーの一致を検査する test は無い（grep で `impl Drop` を読む
  テストは見つからない）。よって本 PBI は新規の契約テストを足す作業。

### 実装手順
1. 変異を手で入れて既存テストが緑のままであることを確認（証拠を残す）
2. `drop_body(path)` を書き、2 ファイルから抽出できることを確認
3. 一致 assert と順序 assert を書く。変異 3 種で赤を確認
4. fixture の自己テスト 2 本を足す
5. tdd.md に 1 句
6. 検証: `cargo test --test scratch_dir_contract`、`cargo test --lib`、fmt、clippy

### 落とし穴
- コメント除去は行頭 `//` だけで足りる（文字列内に `//` は無い）。`///` も同じ扱いで
  よいが、Drop 本体には doc コメントが無い
- `unclaim` は `keep_scratch` の早期 return でも呼ばれる。「最初の unclaim」ではなく
  「最後の unclaim」と remove を比べる
- 2 つの `impl Drop for ScratchDir` は 1 ファイルに 1 つずつ。ブロック抽出は
  `impl Drop for ScratchDir {` から波括弧の対応を数えて閉じる
- fixture に本物の `ScratchDir` を使うと、fixture 自身が guard の Drop を通り、
  検証したい順序 assert と混ざる。fixture は単なる一時ファイル 2 つ（既存の
  `ScratchDir` で作った dir に置く）にする
- 正規化で空白を潰しすぎると、`let _ =` の有無など意味のある差が消える。行末空白と
  インデントだけを正規化する

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 片方だけ順序を変えた変異で赤になる（PR 本文に証拠）
- [ ] 両方同じく壊した場合も順序 assert で赤になる
- [ ] 既存の scratch 契約テストが green のまま
- [ ] tdd.md に 2 コピー契約の 1 句が入っている
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
