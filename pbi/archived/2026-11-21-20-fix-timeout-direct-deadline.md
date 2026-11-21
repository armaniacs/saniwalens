# PBI: --timeout を direct 経路と headless verbose 経路に適用する

## ユーザーストーリー
optimize を `--verbose --timeout 5s` 付きで実行する saniwalens ユーザーとして、
verbose で見ている headless run にもタイムアウトが効き、ハングした advisor が
子プロセスごと回収される状態がほしい。なぜなら spec は「--timeout bounds a
headless run」と約束しているのに、verbose を付けた瞬間にその束縛が黙って消え、
ハングした run が永遠に戻らなくなるからだ。明示した timeout が沈黙のうちに
捨てられていること自体も、ツールへの信頼を損なう。

## ビジネス価値
- 優先度: 順位 06 / 13 件中
- spec 契約の回復: `docs/specs/cli.md:485-488` は「`--timeout <DURATION>`
  (e.g. `30s`, `2m`, `500ms`) bounds a headless run: on expiry the child is
  killed and reaped」と定める。verbose の headless run も「headless run」だが、
  現実には束縛されない。spec と実装の乖離を最も安価に塞ぐパッチ
- ハング時の自律回復: `src/cli/headless.rs:191-193` の設計意図は
  「a hung advisor is an error, not an indefinite wait」。この原則が
  Headless+verbose 経路でのみ破れている
- 沈黙の廃棄の明示化: `resolve_timeout`（headless.rs:237-249）は `--timeout 0`
  のときしか警告せず、明示 `5s` が direct 経路で黙って捨てられる。
  さらに既定 60s（headless.rs:194, :238-240）も Headless+verbose で捨てられる。
  「効かない」ことがユーザーから見えるようになる

## BDD受け入れシナリオ

```gherkin
Scenario: verbose な headless run でも timeout で子プロセスが殺される
  Given ハングする advisor 子プロセスを spawn する合成 fixture がある
  When saniwalens が optimize を --harness opencode --verbose --timeout 5s で実行する
  Then 5 秒以内に run が timeout エラーで終了する
  And 子プロセスは kill され reap 済みで残留しない
  And エラーは raw run log のパスを名指しする

Scenario: verbose な headless run の正常系出力は 1 バイトも変わらない
  Given 正常終了する advisor 子プロセスを spawn する合成 fixture がある
  When 修正前と修正後のバイナリで --verbose を付けて同じ入力を走らせる
  Then stdout がバイト同一である（raw ストリーム中継の形は変わらない）

Scenario: direct 経路では timeout が効かないことが警告で明示される
  Given interactive ハーネスが選ばれている
  When saniwalens が --timeout 5s を付けて optimize を実行する
  Then stderr に「direct 経路では --timeout が効かない」旨の警告が出る
  And advisor の起動自体は通常どおり行われる（deadline で殺されない）

Scenario: --timeout 0 は従来どおり全経路で deadline 無効を意味する
  Given Headless+verbose 経路が選ばれている
  When saniwalens が --timeout 0 を付けて optimize を実行する
  Then 既存の「--timeout 0 disables the deadline」警告が出る
  And run は deadline なしで最後まで走る
```

## 受け入れ基準
- [x] Headless+verbose 経路に deadline 機構が適用され、期限切れで子プロセスが
  kill/reap され、エラーが raw run log を名指しする（spec 契約の回復）
- [x] 既定 60s（`DEFAULT_BRIEFING_TIMEOUT`）も Headless+verbose 経路で効く
- [x] direct（interactive）経路では deadline を適用せず、timeout が効かない
  ことを stderr 警告で明示する（方針は実装アプローチ参照。実装時に
  (a)/(b) の決定を覆す場合は本 PBI の注記に理由を残す）
- [x] verbose 経路の stdout 中継が byte-identity を保つ（正常系で差分なし）
- [x] `resolve_timeout` の警告文変更は help golden に触れない
- [x] hung 子プロセスを spawn する自動テストがある（kill/reap を含む）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体: `parse_timeout` / `resolve_timeout` は既存テスト（headless.rs:691-709 付近）
  が土台。警告の追加は「direct 経路で明示値が捨てられる」ことに対する
  仕様として、経路を知る呼び出し側から注入して固定する
- 単体: Headless+verbose 経路の deadline。hung 子プロセスを spawn する
  既存の headless テスト群（headless.rs:426 以降、timeout 失敗で log を
  名指す検証 :539 付近）が子プロセス処理の前例。期限ちょうど・期限切れ直後・
  `--timeout 0`（deadline なし）の境界を固定する
- 統合: deliver の経路分割（deliver.rs:257-264）を RecordingAdvisor 系の
  テストダブルで検証する。既存の
  `headless_routes_through_the_logged_adapter_unless_verbose`
  （deliver.rs:546 付近）が経路の証人で、verbose+timeout の組合せを追加する
- E2E: 正常系の verbose 表示の byte-identity。修正前後のバイナリで同じ
  合成 fixture を走らせ diff 空。hung fixture は合成のみ（実 transcript・
  実ハーネスを持ち込まない）

## 実装アプローチ
推奨方針は **(a)+(b) の組合せ**。主目的は spec 契約
（`--timeout` bounds a headless run）の回復で、direct は警告明示までとする。

- **(a) Headless+verbose に既存 deadline 機構を適用**: deliver の経路分割で
  `mode == DeliveryMode::Headless && verbose` を direct に流さず、
  headless.rs の deadline 生成（headless.rs:298 付近 `timeout.map(|limit|
  Instant::now() + limit)`）と kill/reap（`wait_for_status` headless.rs:138-170、
  `Drop` 実装 :173-189）を再利用する。verbose は stdout をユーザー端末へ
  そのまま中継するだけなので、piped+reader という headless の輸送手段を
  共用しつつ中継先を変えれば byte-identity は自ずと保てる
- **(b) direct は deadline なし・警告で明示**: direct は「起動してターミナルを
  渡す」モード（interactive TUI。`Command::status()` の無期限ブロッキング待ち、
  deliver.rs:145-155）で、ユーザーが対話中のセッションを殺すのは適切でない
  可能性が高い。このため direct へは timeout を渡さず、resolve_timeout の
  警告で「direct 経路では timeout が効かない」を明示する
- どちらを採るか（特に direct を将来 deadline 対応に上げるか）は実装時に
  決定させてよいが、spec 契約の回復が主目的である順序は変えない

## 見積もり
1日。deadline 機構は headless.rs に既存で、経路の付け替えと警告追加、
hung 子プロセスのテストが実質の作業。

## 技術的考慮事項
- spec 同期: `docs/specs/cli.md` の `--timeout` 節（:485-488 付近）を本変更と
  同じコミットで更新する（spec-sync 規約）。verbose 経路も束縛されること、
  direct 経路では効かないことを spec 側に明記する
- ロールバック不要: 制限（deadline）の追加のみで、既存データや出力形式の
  移行を伴わない。revert 1 コミットで戻る
- トレイト変更の波及: `AdvisorLaunch::direct` に timeout を渡す場合、
  実装ダブル3件（deliver.rs:469 付近 `RecordingAdvisor`、headless.rs:754 付近
  `FailingHeadlessAdvisor`、handoff.rs:655 付近 `DialogRecorder`）のシグネチャが
  全て追従を要する。メソッド追加で済ませるか、シグネチャ変更にするかは
  実装時にダブルの変更量と見比べて決める
- 警告の配置: `resolve_timeout`（mod.rs:538 付近で呼ばれる唯一のチョーク
  ポイント）は経路を知らない。経路情報を引数で渡すか、呼び出し側で
  分岐警告するかは実装時に決める。help golden には触れない

## 実装者向け注記

### 現状コードの確認
- `src/cli/deliver.rs:257-264` — timeout を渡すのは
  `mode == DeliveryMode::Headless && !verbose` の pipe 経路のみ（:258）。
  それ以外は :263 の `advisor.direct(binary, leading, &prompt)` で、
  direct に timeout 引数は存在しない
- `src/cli/deliver.rs:117-125` — `AdvisorLaunch::direct` のトレイト定義。
  `:145-155` — `ProcessAdvisor::direct` は `Command::status()` の
  無期限ブロッキング待ち
- `src/cli/headless.rs` — deadline 実装の所在: 生成 `:298` 付近、
  `wait_for_status` `:138-170`、`next_line` の `recv_timeout` `:403-424`、
  kill/reap は `HeadlessRun::drop` `:173-189`
- `src/cli/headless.rs:237-249` — `resolve_timeout`。フラグ欠落で既定 60s
  （`:194` `DEFAULT_BRIEFING_TIMEOUT`、`:238-240`）、明示 `0` でのみ警告。
  呼び出し箇所は `src/cli/mod.rs:538`
- `src/cli/handoff.rs:85-90` — 拒否されるのは `--apply` + `--verbose` のみ。
  `optimize --harness opencode --verbose --timeout 5s` は普通に parse・実行
  され、その実行が timeout を黙って捨てる（到達確定）

### 実装手順
1. 失敗するテストから: hung 子プロセス fixture で
   Headless+verbose+`--timeout 5s` が timeout エラーになり、kill/reap される
   ことを主張する（headless.rs の既存 hung テストを型にする）
2. deliver の経路分割に Headless+verbose 経路を追加し、deadline を通す。
   verbose の stdout 中継を壊さない（byte-identity テストを並行で固定）
3. direct 経路の警告を追加（「--timeout は direct 経路では効かない」旨）。
   `--timeout 0` の既存警告と重ならない文面にする
4. テストダブル3件を追従させ、全テスト green
5. `docs/specs/cli.md` の `--timeout` 節を同期し、
   cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
   cargo test で仕上げ確認

### 落とし穴
- kill/reap のテストは hung 子プロセスを spawn する。既存の headless テスト群
  （headless.rs:426 以降）が子プロセス処理の前例で、unix 専用 `cfg` の形も
  それに合わせる
- verbose 経路の stdout 中継を壊さないこと。byte-identity は正常系の
  verbose 表示に対する契約で、reader 経由に変えた瞬間に壊れやすい
  （改行正規化・stderr 混線に注意）
- `resolve_timeout` の警告文変更は help golden に触れない。
  警告は stderr への追加のみに留める
- `--timeout 0`（deadline なし明示）の既存警告（headless.rs:243-246）と
  新警告が二重に出ないよう、分岐を一箇所にまとめる
