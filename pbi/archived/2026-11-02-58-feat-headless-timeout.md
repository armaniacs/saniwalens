# PBI: headless に期限を入れ、超過で child を kill + wait して log を名指す

## ユーザーストーリー
利用者として、advisor がハングしても永久に待たず、期限で止まって raw log の場所を
教えてほしい、なぜなら headless には timeout が無く、`read_line` も `child.wait()` も
無期限にブロックするので、1 回のハングが端末ごと固めるからだ。

## 優先度
- 順位: 6 / 全候補数 9
- RICEスコア: 4.0（R=2 / I=2 / C=2 / Effort=2日）
- 根拠: ハングは稀だが起きると復帰手段が無い。I=2 は「待たされる」被害でデータは壊れ
  ない。C=2 は既定値の判断（入れるか、opt-in か）が残るため。E=2日 は stream loop を
  thread に移す設計込み。

## ビジネス価値
- ハングした advisor を期限で止め、raw log の場所を名指すので、原因調査を続けられる。
- 期限を opt-in にすれば、既存の呼び出しの待ち時間と出力は変わらない。
- 測定方法: `--timeout 1s` + 無出力 child で 2 秒以内に Err、直後に `kill -0` が失敗
  し、stderr に log path が出ること。期限内の run の status / log が不変であること。

## 背景（2026-11-02、round4 の timeout 欠落 + PBI 52 の known limitation）
- `src/cli/headless.rs:214-254` の `drive` は `BufReader::lines()` を `for` で回す。
  行が来ないハングでは `read_line` が永久にブロックし、期限を測る場所が無い。
- `:143-157` の `wait_for_status` は `child.wait()`。child が exit しなければ永久に
  待つ。`:158-171` の Drop は kill + wait するので、期限超過時に kill する部品は既に
  ある（Drop がやることを明示的に前倒しすればよい）。
- PBI 52 の known limitation: `Child::kill` は直接の child だけを殺し、opencode が
  spawn した孫 process は残り得る。プロセスグループ化は本 PBI の範囲外。
- 現状の無期限待ちは「正当に長い run を殺さない」ための暗黙の既定でもある。既定値を
  入れると CI の遅い run を落とす危険があるため、既定の扱いを決める必要がある。
- 根本原因: 期限という概念が run の状態機械に無く、待ちが 2 箇所（行読みと wait）に
  分かれている。

## BDD受け入れシナリオ

```gherkin
Scenario: 期限超過で止まり log を名指す
  Given --timeout 1s と、何も出力しない child
  When run_headless が期限を超える
  Then child は kill + wait され、error は raw log のパスを含む
  And heartbeat thread は止まっている

Scenario: 期限内に終わる run は不変
  Given --timeout 10s と即終了する child
  When run_headless が終わる
  Then status と log path が返り、stdout / stderr は現行と同一

Scenario: 期限を指定しなければ従来どおり
  Given --timeout を渡さない
  When 長い run が続く
  Then 期限は無く、既存 test の待ち時間は変わらない
```

## 受け入れ基準
- [ ] 既定の扱いを決めて spec に書く。推奨: 既定は無期限（既存の byte identity と
  「正当に長い run を殺さない」を守る）、`--timeout <span>` を opt-in。既定を入れる
  場合は値を help・`docs/specs/cli.md`・`docs/specs/harness.md` に明記
- [ ] `optimize` に `--timeout <span>` を追加する（`src/cli/mod.rs:223-246` の
  `Command::Optimize`）。値は `30s` / `10m` / `2h` の形。`--last` の span parser とは
  別物（`7d` を受けたくない）なので専用の小さな parser を置き、単体テストを付ける
- [ ] 期限は spawn から exit までの全体に掛ける。行読みがブロックするので、読み取りを
  thread に移して `recv_timeout` で待つ（heartbeat と同じ構造）。期限超過は
  stop -> `wait_for_status` ではなく kill + wait -> error
- [ ] error は `raw output: <path>`（既存の prefix）と期限値を含む
- [ ] `--timeout` は headless のみ有効（`--print` と interactive では無効）であること
  を help か spec に書く
- [ ] `docs/specs/harness.md:156-166` に「期限超過は kill + wait + raw log、孫 process
  は残り得る」を明記。`docs/specs/cli.md:469-475` の synopsis に `[--timeout <span>]`
- [ ] Red テスト: 超過（1s で 2 秒以内に Err）/ 期限内（status と log が不変）/
  無指定（期限なし）。`tests/headless_timeout.rs` を新設するか `tests/headless_seam.rs`
  に足す
- [ ] kill 後の `wait` を必ず呼ぶ（zombie を残さない）。Drop の二重 kill が起きない
  こと（`wait_for_status` が成功したら child を None にする既存契約 `:143-157`）
- [ ] fmt、clippy、フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `/bin/sh -c 'sleep 30'` を child にして `--timeout 1s`。2 秒以内に Err、`kill -0`
  で child が死んでいること、stderr に log path が出ること
- 期限内の run（`/bin/sh -c 'printf hello'`）で status 0 と log 内容が不変
- 無指定で既存の headless の e2e が不変

### 統合テスト
- `tests/headless_seam.rs` の double 経由で、期限超過が heartbeat を止めること
- `--print` と interactive では `--timeout` が無視されること（parse の段で拒否するか、
  spec どおり無視するかを pin）

### 単体テスト
- span parser: `30s` / `10m` / `2h` / `0s`（拒否）/ `10x`（拒否）/ 空
- 期限の境界: 期限ちょうど、期限 1ms 前（`Instant` の単調性に依存する部分は
  `recv_timeout` に閉じる）

## 実装アプローチ
- **Outside-In**: 無出力 child の e2e で「10 秒待っても返らない」ことを確認（赤）
  -> 期限を `drive` に通す -> 読み取りを thread 化 -> kill + wait -> error に log と
  期限 -> spec 2 節。実装は subagent-driven-development で行う。

## 見積もり
2 SP（E=2日。RICE 4.0）

## 技術的考慮事項
- 依存関係: PBI 52 の着地後（`HeadlessRun` の所有関係が前提）。54 / 55 と同じファイル
  を触るので直列（55 の後）
- 孫 process の残余リスクは消えない。プロセスグループ kill（`setsid` / `killpg`）は
  別 PBI とし、本 PBI は spec に明記するところまで
- 期限は `Instant`（単調時計）で測る。wall clock は使わない
- 読み取り thread 化で `readable` フラグと `count_event` の共有が必要。heartbeat と
  同じ `Arc<AtomicUsize>` の流儀に合わせる
- 非機能要件: プライバシー（log は temp の 0600、期限 error にはパスだけ載せる）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/cli/headless.rs:184-210` `run_headless`、`:214-254` `drive`（`:237-250` の
  行読みループ）、`:252` の wait、`:143-157` `wait_for_status`、`:158-171` Drop。
- `src/cli/mod.rs:223-246` `Command::Optimize`（`--print` / `--verbose` の隣に
  `--timeout` を足す）。
- `docs/specs/harness.md:156-166`（headless 節）、`docs/specs/cli.md:467-475`
  （optimize synopsis）、`:543-560`（delivery 節）。
- 既実装の確認: timeout を示す識別子は無い（grep で `timeout` は optimize の説明文
  以外にヒットしない）。よって本 PBI は新規の期限機構を足す作業。

### 実装手順
1. Red: `--timeout 1s` + 無出力 child が 2 秒以内に戻らないことを確認
2. 専用 span parser（`30s` / `10m` / `2h`）と単体テスト
3. 読み取りを thread に移し、`recv_timeout` で期限を測る（行 / 終了 / 期限の 3 値）
4. 期限超過で stop -> kill -> wait -> `raw output:` つき error
5. spec 2 節（harness と cli）と help を更新
6. 検証: 新 e2e 3 本、`tests/headless_seam.rs`、フルスイート、`kill -0` の確認

### 落とし穴
- kill した後に `wait` を忘れると zombie が残る。`wait_for_status` は成功時だけ child
  を None にするので、kill 経路では明示的に wait する
- 読み取り thread を join しないまま返すと、log file への書き込みが error 後に続く。
  期限超過では thread を join するか、log を閉じる順序を決める
- `recv_timeout` の期限は「最後の行から」ではなく「spawn から」で測る。行が来続ける
  run は期限で切らない、という別解もあるが、本 PBI は全体期限を採る（spec に明記）
- `--last` の parser を流用すると `7d` を受け付けてしまう。専用 parser にする
- 既定 timeout を入れると CI の遅い run が落ちる。既定無期限の根拠を spec に残す
- `--print` は process を起動しないので期限は無関係。混ぜない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 期限超過で child が reap され、error が raw log と期限を名指す
- [ ] 期限内の run の status / log / 出力が不変
- [ ] `--timeout` 無指定の挙動が不変（既定の判断が spec にある）
- [ ] harness.md と cli.md 更新済み（孫 process の残余リスクを含む）
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
