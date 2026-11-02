# PBI: headless run の thread と child の寿命を RAII guard に閉じる

## ユーザーストーリー
利用者として、headless の advisor が途中で失敗しても child process が孤児として
残らず、heartbeat も止まってほしい、なぜなら `run_headless` の 5 つの `?` が
正常路末尾の stop/join を迂回し、child の Drop は kill も wait もしないからだ。

## 優先度
- 順位: 10 / 全候補数 10
- RICEスコア: 5.3（Reach=2 / Impact=2 / Confidence=2 / Effort=1.5日）
- 根拠: 発生頻度は低いが、child 孤児は持続的でユーザーが気づけない。mod.rs の
  3,100 行予算に余白が無いため、run_headless を新 module へ移す移動込みで閉じる。
  他 PBI が mod.rs を触った後に置き、budget を最後に再測定する。

## ビジネス価値
- 失敗経路でも child が reap され、heartbeat thread が停止する。
- エラーメッセージに raw log のパスが必ず載り、失敗の診断ができる。
- 測定方法: 不正 UTF-8 で `Err` になった直後に child が死んでいること。
  spawn 失敗で thread が残らないこと。成功路の status/log が不変であること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 13本）
- `src/cli/mod.rs:731-764` の `run_headless` は heartbeat を spawn した後、
  `launch.spawn()?`、`stdout.take()?`、`line?`、`writeln!?`、`child.wait()?` の
  5 つの `?` で早期 return する。stop.store と heartbeat.join は正常路の末尾にしかない。
- `Child` の Drop は kill も wait もしない。probe p3 で、不正 UTF-8 の `Err` の瞬間に
  child が生存していることを確認。p11 で、reader を drop しても child が走り続ける
  ことを確認。p9 の guard 試作では、error 後に beat 0・child 死亡・teardown が
  poll 以内に収まった。
- beat 自体は単発 CLI では観測できない（error 後数 ms で process 終了、CADENCE=30s）。
  危険なのは child 孤児。heartbeat だけ止める修正は孤児を「静かな孤児」に変えて悪化する。
- 根本原因: 獲得した 2 つのスコープ資源（thread と child）の後始末を、スコープ終了
  ではなく正常路の末尾の文として書いたこと。

## BDD受け入れシナリオ

```gherkin
Scenario: 走行中の失敗で child が reap される
  Given headless の child が不正 UTF-8 を出して lines() が Err になる
  When run_headless が Err を返す
  Then child は kill + wait され、heartbeat thread も終了している

Scenario: spawn 失敗で資源が残らない
  Given binary が存在しない
  When run_headless が Err を返す
  Then heartbeat thread は残らず、beat も送られない

Scenario: 成功路は不変である
  Given 正常な child が status 0 で終わる
  When run_headless が Ok を返す
  Then status と log path が返り、log に raw 行が書かれている
```

## 受け入れ基準
- [ ] `src/cli/headless.rs` を新設し、`run_headless` と `HeadlessRun` guard を移す
- [ ] `HeadlessRun` は stop / child / heartbeat を所有し、Drop で
  「stop -> child.kill + wait -> heartbeat.join」の順に後始末する。
  Drop 内で panic しない（すべて `let _ =`）
- [ ] `src/cli/mod.rs` は `pub mod headless;` と `deliver` からの 1 呼び出しだけを持つ。
  `run_headless` の移動で mod.rs は約 77 行縮むため、同一 PR で budget を実測して
  下げる（zero-slack、spec 更新と同一 PR）
- [ ] エラーに raw log のパスを `with_context` で載せる
- [ ] Red テスト 3 本: 走行中失敗で child が死ぬ、spawn 失敗で beat が残らない、
  成功路の status/log が不変
- [ ] guard の単体テスト: Drop で thread が止まり、child が reap される
- [ ] spec 更新: `docs/specs/harness.md` の headless 節に「every exit stops the beat
  and reaps the child; a failure names the raw log」、`docs/specs/cli.md` の
  delivery 節に run log の寿命
- [ ] `tests/cli_mod_line_budget.rs` の budget を実測値に更新する
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 不正 UTF-8 を出す `/bin/sh` の child で、Err 後に `kill -0` が失敗すること
- 存在しない binary で Err になり、beat が 0 であること
- 成功路の status 0 と log path の返却

### 統合テスト
- guard の Drop が poll 以内に thread を止めること
- `deliver` の headless 経路が新しい `run_headless` を呼ぶこと

### 単体テスト
- `HeadlessRun` の Drop 順（stop -> child -> heartbeat）
- 二重 drop で panic しないこと

## 実装アプローチ
- **Outside-In**: 不正 UTF-8 の Red テスト -> run_headless の移動（挙動不変）->
  guard 導入 -> context -> spec / budget。実装は subagent-driven-development で実施する。

## 見積もり
1.5 SP（E=1.5日。RICE 5.3）

## 技術的考慮事項
- 依存関係: ④⑩ の後に置く（mod.rs の行数と budget を最後に確定させる）
- `Child` の Drop は kill/wait しない。guard が明示的に所有する
- 孫 process は kill できない。残余リスクとして記録する
- error path に最大 poll（500ms）の teardown が加わる。許容する
- `Drop` 内の panic は abort になる。`unwrap` / `expect` を使わない
- 非機能要件: プライバシー（raw log は 0600 の temp、内容はコミットしない）

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `src/cli/mod.rs:698-775` の `run_headless`、:731-736 の spawn、:743-762 の 5 つの `?`。
- `src/cli/heartbeat.rs:23-58` の stop 契約と poll。
- `tests/headless_seam.rs` の既存テスト（① が同じファイルを触る）。
- `tests/cli_mod_line_budget.rs:19` の budget（現在 3,100）。
- `docs/specs/harness.md:145-191` の headless 節、`docs/specs/cli.md:455-475` の delivery 節。

### 実装手順
1. Red: 不正 UTF-8 で child が生き残ることを確認
2. `run_headless` を `src/cli/headless.rs` へ移動（挙動不変、既存テスト green）
3. `HeadlessRun` guard を導入し、5 つの `?` が Drop を通るようにする
4. `with_context` で log path を載せる
5. spec と budget を同一コミットで更新
6. 検証（headless_seam、budget、fmt/clippy、フルスイート）

### 落とし穴
- heartbeat だけ止めると child 孤児が静かになる。guard は child も所有する
- Drop 内で `unwrap` しない。panic は abort
- guard のフィールド順（child を heartbeat より先）を意識する
- mod.rs の budget を上げない。移動で下げる
- ① のテスト変更と衝突しないよう、① の後に rebase する

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 失敗経路で child が reap され、beat が残らない
- [ ] 成功路の status/log が不変
- [ ] mod.rs の budget が実測値に更新されている
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] harness.md と cli.md 更新済み
