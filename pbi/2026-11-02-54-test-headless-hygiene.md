# PBI: headless 失敗系 test の log 後片付けと StopOnDrop の join

## ユーザーストーリー
コントリビュータとして、headless の失敗系テストが temp に log を残さず、heartbeat の
テストが panic しても thread を確実に join してほしい、なぜなら失敗系 2 本は
`run_headless` の private log を消さず、`StopOnDrop` は stop を立てるだけで join しない
からだ。

## 優先度
- 順位: 2 / 全候補数 9
- RICEスコア: 6.0（R=1 / I=1 / C=3 / Effort=0.5日）
- 根拠: 影響はテストの後片付けと test binary の stderr 汚染に閉じるが、原因は 1 つ
  （成功路だけが log を消し、guard が handle を持たない）で確信度が高い。PBI 54 の
  範囲は `src/cli/headless.rs` の test module と `tests/headless_seam.rs` のみ。

## ビジネス価値
- `cargo test` を回すたびに `$TMPDIR/saniwalens-run*` が増えない。PBI 50 の scratch
  dir guard と同じ「テストは自分が出したものを消す」規約に headless も揃う。
- panic したテストが heartbeat thread を残さないので、test binary の stderr が他の
  テストの観測を汚さない。
- 測定方法: `cargo test` 前後で `$TMPDIR` の `saniwalens-run*` の数が同じ。panic を
  挟んだ `tests/headless_seam.rs` の実行後も beat が出ないこと。

## 背景（2026-11-02、round4 最終 whole-branch review M7 / M8）
- `src/cli/headless.rs:269-304` の `a_bad_utf8_line_reaps_the_child_and_names_the_log`
  は child の pid file を消す（`:287`）が、`run_headless` が作った log は消さない。
  `:306-320` の `a_spawn_failure_leaves_no_child_and_no_beat` も同じ。成功路
  `:322-336` だけが `:334` で `remove_file` している。
- 消せない理由は型にある。`run_headless` は成功時 `(ExitStatus, PathBuf)` を返すが、
  失敗時は `anyhow::Error` だけで、log path は context 文字列
  `raw output: <path>`（`:196`）にしか存在しない。test から path を取り出す API が無い。
- `tests/headless_seam.rs:109-116` の `StopOnDrop` は `self.0.store(true)` だけを行う。
  Drop は join しない。panic すると `:184` の明示 join に到達せず、thread は test
  binary が終わるまで beat を出し続ける。`:176-183` のコメント自身が
  「disconnect は stop 尊重の終了と早期自滅を区別できない」と認めている。
- 根本原因: 2 つとも「資源の後始末を、テスト本体の末尾の文に置いた」こと。PBI 52 が
  production で直したのと同じ形が test 側に残っている。

## BDD受け入れシナリオ

```gherkin
Scenario: 失敗系 test が log を残さない
  Given 不正 UTF-8 を出す child、または存在しない binary
  When 失敗系 unit test が終わる
  Then run_headless が作った private log は削除されている
  And TMPDIR の saniwalens-run* がテスト前後で増えない

Scenario: panic しても heartbeat thread が残らない
  Given StopOnDrop が live な heartbeat handle を保持している
  When test body が panic する
  Then Drop が stop を立て、deadline 内に thread の終了を観測して join する
  And Drop 自身は panic しない

Scenario: stop 尊重と早期自滅を区別する
  Given 対象の beat が届いた後も thread が生きている
  When stop を立てる
  Then deadline 内に終了する
  And 対象 beat の後にさらに 1 beat 届くことを前提として検査する
```

## 受け入れ基準
- [ ] `raw output: ` の prefix を module 定数 1 つにし、`run_headless` の context
  （`src/cli/headless.rs:196`）と test 用の読み取りが同じ定数を使う。format 側と
  reader 側が別リテラルだと、prefix を変えたときに静かに壊れる
- [ ] error から log path を取り出す module-private helper（`#[cfg(test)]`）を足す。
  `anyhow::Error` の `chain()` を走査し、prefix で始まる context から `PathBuf` を
  返す。`to_string()` は最外の context しか返さないので使わない
- [ ] helper 自身に単体テストを 1 本置く（prefix を変えたら赤くなる形）。加えて
  `err.chain()` に log path が無い場合に `None` を返すことを 1 本
- [ ] 失敗系 2 本（`:269`、`:306`）が helper で log を取り出し、`remove_file` する。
  成功路（`:334`）と同じ後片付けに揃える
- [ ] `tests/headless_seam.rs` の `StopOnDrop` が `Arc<AtomicBool>` +
  `Option<JoinHandle<()>>` + deadline を持つ。Drop は stop -> `is_finished()` を
  deadline まで回す -> `join()`。deadline 超過は stderr に 1 行だけ出し、panic しない
- [ ] テスト本体は handle を `StopOnDrop` に渡し、`:184` の明示 join を guard に寄せる
  （二重 join しない）。stop を立てる箇所（`:169`）も guard 経由にする
- [ ] 早期自滅の検知: 対象 beat（count == 3）の後にさらに 1 beat 届くことを確認して
  から stop を立てる。この 1 beat が「loop はまだ生きている」の証拠になる
- [ ] 既存の `dropping_the_guard_stops_the_beat`（`src/cli/headless.rs:338`）は不変
- [ ] `cargo test --lib`、`cargo test --test headless_seam`、fmt、clippy が green。
  `$TMPDIR` の `saniwalens-run*` がテスト前後で増えないこと

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `cargo test` の前後で `$TMPDIR/saniwalens-run*` を数える（手動確認）。
  `headless_seam` を 3 回連続で回して増えないことを記録する
- panic 経路: `catch_unwind` で test body の panic を捕まえ、guard の Drop 後に beat が
  出ていないことを stderr の capture で確認する

### 統合テスト
- `tests/headless_seam.rs` の `StopOnDrop` が panic 時に deadline 内で join する
- stop 前に thread が生きている（対象 beat の後に 1 beat 届く）ことの観測

### 単体テスト
- 失敗系 2 本が log を消す（`$TMPDIR` を汚さない）
- log path 抽出 helper: 見つかる / 見つからない / prefix 変更で赤
- `StopOnDrop` の Drop が panic しない（二重 drop 相当を含む）

## 実装アプローチ
- **Outside-In**: まず「失敗系 2 本が log を残す」ことを tmpdir の観測で赤にする ->
  prefix 定数と抽出 helper -> 失敗系 2 本を直す -> `StopOnDrop` を handle 所有に変える
  -> 早期自滅の観測を足す。実装は subagent-driven-development で行う。

## 見積もり
0.5 SP（E=0.5日。RICE 6.0）

## 技術的考慮事項
- 依存関係: PBI 52 の着地後（同じ `src/cli/headless.rs` を触るため直列）。他 PBI とは
  ファイルが重ならない
- error を typed error（`HeadlessError { log, source }`）にする案は、PBI 57 の
  delivery-return と戻り型が絡むので本 PBI では採らない。prefix 定数 + 抽出 helper の
  最小案にし、57 が typed 化を選んだら寄せる余地を注記する
- `is_finished()` は Rust 1.61+（Cargo.toml は edition 2024）。`join()` に timeout は
  無いので、deadline は `is_finished()` の spin で測る
- Drop 内で panic しない。`expect` / `unwrap` を使わない
- 非機能要件: log は 0600 の temp。内容は読まずに削除するだけ（プライバシー影響なし）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/cli/headless.rs:184-210` の `run_headless`、`:196` の context 生成、`:257`
  `mod tests`、`:261` `fast_beat`、`:269` 不正 UTF-8、`:306` spawn 失敗、`:322` 成功路、
  `:334` の `remove_file`、`:338` guard の Drop テスト。
- `src/cli/headless.rs:68-107` の `HeadlessRun::start`、`:69` の
  `private_temp_path("saniwalens-run", "log")`。
- `tests/headless_seam.rs:109-116` の `StopOnDrop`、`:134` の生成、`:169` の
  `stop.store`、`:176-183` のコメント、`:184` の join。
- 既実装の確認: log を消しているのは成功路 1 本だけ（`:334`）。`StopOnDrop` に handle
  は無い。よって本 PBI は新規の後片付けを足す作業。

### 実装手順
1. Red: 失敗系 2 本を走らせ、`$TMPDIR` に `saniwalens-run*` が増えることを確認
2. `RAW_LOG_CONTEXT` 定数と `raw_log_path(&anyhow::Error) -> Option<PathBuf>` を足し、
   helper の単体テスト 2 本
3. 失敗系 2 本で log を削除
4. `StopOnDrop` を handle 所有に変え、Drop で stop -> deadline 待ち -> join
5. 対象 beat の後に 1 beat 届くことを観測してから stop
6. 検証: `cargo test --lib`、`cargo test --test headless_seam` を 3 回、tmpdir の増加 0

### 落とし穴
- `anyhow::Error::to_string()` は最外の context だけ。必ず `chain()` を先頭から走査する
- prefix を `format!` と reader で二重に書かない。定数 1 つにする
- `StopOnDrop` の Drop で `join()` を無条件に呼ぶと、thread が止まらないときに test が
  永久に待つ。`is_finished()` の deadline を先に置く
- 明示 join（`:184`）を残したまま guard にも handle を渡すと二重 join で panic する。
  どちらか一方に寄せる
- `sleep` の値を小さくしすぎると早期自滅の観測が flaky になる。cadence 1ms / poll 5ms
  の既存値と `OBSERVE`（2 秒）の予算内で、追加の 1 beat を待つ
- `$TMPDIR` の観測はテスト並列で揺れる。単体の検証は「その test が作った log path が
  消えている」に寄せ、ディレクトリ全体の比較は手動確認にする

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 失敗系 2 本が log を残さない
- [ ] `StopOnDrop` が panic 時に deadline 内で join する
- [ ] stop 尊重と早期自滅を区別する assert が入っている
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
