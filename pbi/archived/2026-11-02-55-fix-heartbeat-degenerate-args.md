# PBI: Beat の退化引数（cadence 0 / poll 0）を構築時に拒む

## ユーザーストーリー
保守担当として、headless の `Beat` に 0 を渡したら busy loop や panic に化けず、構築時
に拒否されてほしい、なぜなら crate-private の test seam が `Duration` をそのまま受け、
0 が入ると `sleep(0)` の tight loop か毎 tick beat の spam になるからだ。

## 優先度
- 順位: 3 / 全候補数 9
- RICEスコア: 6.0（R=1 / I=1 / C=3 / Effort=0.5日）
- 根拠: production 経路は `Beat::default()` の 1 つだけで現状の実害は無い。ただし
  `Beat` は `pub(crate)` の test seam で、`heartbeat::spawn` は `pub` のまま
  `Duration` を受ける。入口が 2 箇所しかないので型で閉じきれる（C=3）。

## ビジネス価値
- 退化した cadence / poll が「静かに別の走り方」をせず、構築時に名指しで落ちる。
- test seam の誤用が、CPU を焼く flaky test ではなく 1 行のエラーになる。
- 測定方法: `Beat::new(ZERO, POLL)` と `Beat::new(CADENCE, ZERO)` が `Err` になり、
  message が 0 の field を名指すこと。

## 背景（2026-11-02、round4 最終 review の派生）
- `src/cli/headless.rs:30-45` の `Beat { cadence, poll }` は `Default` で
  `heartbeat::CADENCE`（30 秒、`src/cli/heartbeat.rs:23`）と `POLL`（500ms、`:26`）を
  入れる。test は `fast_beat()`（`src/cli/headless.rs:261-266`）で 1ms / 5ms に縮める。
- `heartbeat::spawn`（`src/cli/heartbeat.rs:58-84`）の loop は stop 判定 ->
  `sleep(poll)` -> `take_beat(elapsed, &mut next, cadence)`。`next` の初期値は
  `cadence`、`take_beat`（`:34-40`）は `elapsed >= next` で beat を決める。
- cadence が 0 なら `next = 0` になり、`elapsed >= 0` が常に真。30 秒 cadence の設計が
  poll 間隔（既定 500ms）の beat spam に化ける。poll が 0 なら `sleep(0)` で CPU を
  焼く。両方 0 なら「回るほど beat を出す」tight loop になる。
- 呼び出しは 2 箇所だけ: `src/cli/headless.rs:93` と `tests/headless_seam.rs:128`。
  どちらも `Beat` を経由するが、`spawn` 自身は `Duration` を受けるので seam の入口が
  2 つある。
- 根本原因: 非ゼロという不変条件に所有者が無い。定数の doc コメントが唯一の表現で、
  Duration を運ぶ型は 0 を受け取れる。

## BDD受け入れシナリオ

```gherkin
Scenario: 退化した cadence を拒む
  Given Beat::new(Duration::ZERO, POLL)
  When 構築する
  Then Err になり、message が cadence の 0 を名指す

Scenario: 退化した poll を拒む
  Given Beat::new(CADENCE, Duration::ZERO)
  When 構築する
  Then Err になり、message が poll の 0 を名指す

Scenario: 正常な値と既定は不変
  Given Beat::new(1ms, 5ms) と Beat::default()
  When 構築する
  Then 前者は Ok、後者の cadence / poll は production 定数と一致する
```

## 受け入れ基準
- [ ] `Beat::new(cadence: Duration, poll: Duration) -> Result<Self>` を足し、
  `Duration::ZERO` を両 field で拒否する。message は field 名（cadence / poll）と
  受け付ける下限を含める
- [ ] `Beat::default()` は production 定数から同じ構築経路を通る。`new` が `Result` な
  ので、`Default` は定数が非ゼロであることを 1 箇所で表明する（`expect` を置くなら
  message に「production constants are non-zero」と書き、test で守る）
- [ ] `fast_beat()`（`src/cli/headless.rs:261-266`）を `Beat::new(...).expect(...)` に
  置き換える
- [ ] seam の入口を 1 つにする。`heartbeat::spawn` が `Beat` を受け取る形に変え、
  `src/cli/headless.rs:93` と `tests/headless_seam.rs:128` を更新する（代替:
  signature を変えず doc と `debug_assert!` で非ゼロを契約化。推奨は型で受ける方）
- [ ] 赤くなる test 3 本: cadence 0 / poll 0 / 既定の非ゼロ一致
- [ ] `docs/specs/harness.md:156-166` の headless 節に「cadence と poll は非ゼロ」を 1 句
- [ ] clamp 案（0 を最小値に丸める）は採らない。静かに別の走り方を許すと、seam の誤用
  が観測できなくなる。採る場合は spec と test で丸めを明示する
- [ ] fmt、clippy、`cargo test --lib`、`cargo test --test headless_seam` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- なし（process を跨がない。`spawn` の signature 変更は統合テストが検出する）

### 統合テスト
- `tests/headless_seam.rs:128` が新しい signature で `spawn` を呼び、既存の
  first-beat の観測（0 -> 3 の 2 段）が green のまま
- `Beat` を渡す経路で、cadence 0 を作れないこと（型が `Result` を強制する）

### 単体テスト
- `Beat::new(ZERO, POLL)` が `Err`、message に `cadence`
- `Beat::new(CADENCE, ZERO)` が `Err`、message に `poll`
- `Beat::default()` の 2 field が `heartbeat::CADENCE` / `POLL` と一致
- `fast_beat()` が `Ok` を返す（1ms / 5ms）

## 実装アプローチ
- **Outside-In**: 退化値の Red テスト -> `Beat::new` -> `fast_beat` の置き換え ->
  `spawn` の signature を `Beat` 受けに変える -> spec 1 句。実装は
  subagent-driven-development で行う。

## 見積もり
0.5 SP（E=0.5日。RICE 6.0）

## 技術的考慮事項
- 依存関係: PBI 52 / 54 と同じ `src/cli/headless.rs` と `tests/headless_seam.rs` を
  触るので直列（54 の後）
- `Duration::ZERO` は Rust 1.53+。Cargo.toml は edition 2024 なので利用可能
- `heartbeat::spawn` の signature 変更は 2 箇所だけ（grep 済み）。`Beat` は
  `pub(crate)`、`heartbeat` は `pub mod`（`src/cli/mod.rs:31`）なので、`Beat` を
  `pub` にするか `spawn` を `pub(crate)` に下げるかの判断が要る。推奨: `Beat` を
  `pub` にして seam の型を 1 つにする
- `Duration` の下限を 1ms にする案は、正当な 1ms 未満の test を将来禁じる。0 だけを
  拒む方が意図が明確
- 非機能要件: なし（メモリ・I/O に影響しない）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/cli/headless.rs:30-45` の `Beat` と `Default`、`:68` の `start(beat: Beat)`、
  `:93` の `heartbeat::spawn` 呼び出し、`:184-190` の `run_headless` 引数、`:261-266`
  の `fast_beat`。
- `src/cli/heartbeat.rs:23` `CADENCE`、`:26` `POLL`、`:34-40` `take_beat`、
  `:42-48` `next_deadline`、`:58-84` `spawn`（loop 本体）。
- `tests/headless_seam.rs:128` の `spawn` 呼び出し（cadence 1ms / poll 5ms）。
- 既実装の確認: 0 を弾く検査は無い（grep で `is_zero` は見つからない）。`Beat` の
  variant は 1 つだけ。よって本 PBI は不変条件を足す作業。

### 実装手順
1. Red: `Beat::new(ZERO, POLL)` が `Ok` になってしまうこと（`new` が無いので
   コンパイルできない）を確認し、先に test を書く
2. `Beat::new` を実装（0 を拒否、field 名つき message）
3. `Default` を `new` 経由にし、`fast_beat` を置き換え
4. `heartbeat::spawn` を `Beat` 受けに変え、2 箇所の呼び出しを更新
5. harness.md に 1 句
6. 検証: `cargo test --lib`、`cargo test --test headless_seam`、fmt、clippy

### 落とし穴
- `Default` を `new(...).unwrap()` にすると production に panic 経路が入る。定数の
  非ゼロを 1 箇所で表明し、その表明自体を test で守る
- 0 を clamp すると、`Beat::new(0, 0)` が「動くが別物」になる。test seam の誤用は
  静かに直すより落とす方がよい
- `spawn` の signature を変えると `tests/headless_seam.rs` の既存 test が型で落ちる。
  これは意図した赤なので、テストの意味を変えずに直す
- cadence が poll より小さい既存の縮小（1ms / 5ms）は退化ではない。0 だけを拒む
- `pub mod heartbeat` の公開範囲を広げすぎない。`Beat` を `pub` にするなら
  `src/cli/mod.rs` の再輸出は不要（`cli::headless::Beat` で届く）

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 0 の cadence / poll が構築時に拒否される
- [ ] seam の入口が 1 つ（`Beat`）になっている
- [ ] harness.md に非ゼロの契約が 1 句入っている
- [ ] fmt、clippy、対象テスト、フルスイートが green
- [ ] コードレビュー完了
