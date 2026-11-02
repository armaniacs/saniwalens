# PBI: deliver が配信結果を返すか、現状維持かを spike で決める

## ユーザーストーリー
CLI の呼び出し側として、`deliver` が Print / 起動 / 失敗のどれになったかを返して
ほしい、なぜなら今は `Result<()>` で成功時の status も log path も捨てており、呼び出し
側が結果を見て表示や後続処理を選べないからだ。ただし使い道が無いなら、型を増やさず
現状維持と決めたい。

## 優先度
- 順位: 5 / 全候補数 9
- RICEスコア: 4.0（R=3 / I=2 / C=2 / Effort=3日）
- 根拠: R=3 は全 delivery 経路の戻り型を変えるため。I=2、C=2 は「消費者が未確定」
  （`optimize` は現状 status を使わない）。round3 から 3 回先送りされており、決着を
  つけること自体に価値がある（spike 0.5 日 + 実装 2.5 日）。

## ビジネス価値
- 配信結果（Print / 起動 / 失敗）が型で表され、呼び出し側が「成功したが status は 1」
  のような状態を扱えるようになる、または「要らない」と記録して探索を閉じる。
- どちらに転んでも `docs/specs/cli.md` の delivery 節が判断を持つ。
- 測定方法: `--print` の stdout bytes、quiet / verbose headless の stderr bytes、
  失敗時の message が現行と同一であること。spike の結論が PR 本文と spec に残ること。

## 背景（2026-11-02、round3 の delivery-return の再持ち越し）
- `src/cli/mod.rs:561-614` の `deliver` は `Result<()>`。成功時は `status` を捨て、
  失敗時だけ `anyhow::bail!`（`:606-611`）で終わる。呼び出し側は `:686` の 1 箇所で
  `deliver(plan, &mut ProcessAdvisor)?` としか使わない。
- 材料はすでに揃っている: `AdvisorLaunch::direct`（`:505-513`）は `ExitStatus` を、
  `headless`（`:515-520`）は `(ExitStatus, PathBuf)` を返す。`deliver` が捨てている。
- 台帳の 214-215 行が「delivery-return（RICE 4.0 ― PBI 39 の着地後、次の view 着地 PR
  に同乗）」として先送りし、round3 / round4 の 2 回のレビューでも非ブロッキングのまま
  残った。先送りの理由は毎回「消費者が無い」で、その確認が記録されていない。
- 根本原因: 戻り型の是非を「型の一般論」で議論してしまい、呼び出し側が何に使うかを
  先に決める手順が無かった。spike で消費者を列挙すれば 0.5 日で決着する。

## BDD受け入れシナリオ

```gherkin
Scenario: Print の結果が返る
  Given DeliveryPlan::print
  When deliver が成功する
  Then 結果は「stdout に書いた」を表し、status も log も持たない
  And stdout の bytes は現行と同一

Scenario: 起動の結果が返る（spike が消費者を見つけた場合）
  Given launch plan と status 0 を返す recording double
  When deliver が成功する
  Then 結果は起動で、mode と ExitStatus を持つ
  And stdout / stderr の bytes は現行と同一

Scenario: 失敗は今までどおり Err
  Given status 1 を返す double
  When deliver を呼ぶ
  Then Err で、message は不変（raw output を含む経路も同じ）
```

## 受け入れ基準
- [ ] spike（timebox 0.5 日）: 呼び出し側が結果を使って何を変えるかを 3 案で比較し、
  判断を PR 本文に記録する。(a) `DeliveryOutcome` enum を返す、(b)
  `Result<(DeliveryMode, ExitStatus)>` を返す、(c) 現状維持
- [ ] 消費者が無いなら (c) を結論にし、`docs/specs/cli.md:543-560` の delivery 節に
  「戻り型は `Result<()>` のまま。結果を必要とする消費者が現れた時点で再評価する」と
  1 文足す。型は足さない
- [ ] 消費者があるなら (a) を採り、`pub enum DeliveryOutcome { Printed, Launched {
  mode, status }, Headless { status, log } }` を追加して
  `deliver -> Result<DeliveryOutcome>` にする。既存の `?` と `bail!` は変えない
- [ ] 出力 byte 同一: `--print` の stdout、quiet / verbose headless の stderr、失敗
  message（`raw output:` を含む経路と含まない経路の両方）を capture で比較する
- [ ] test double の更新は `src/cli/mod.rs:2818-2890` の 4 本と
  `tests/headless_seam.rs:89` のみ。double の記録内容は増やさない
- [ ] `#[must_use]` を付けるなら呼び出し側で明示的に扱う（`let _ =` で黙らせない）。
  付けないなら doc に「無視してよい」と書く。`let _ =` を許すなら型を足す意味が無い
  ので、(a) を採る場合は `optimize` が結果を 1 箇所で使う形にする
- [ ] `docs/specs/cli.md` の delivery 節を結論に合わせて更新（(a) なら戻り値の意味、
  (c) なら現状維持の理由）
- [ ] fmt、clippy、フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `saniwalens optimize --print` の stdout がバイト同一（既存の capture があれば流用）
- 存在しない binary での失敗 message がバイト同一

### 統合テスト
- recording double 経由で 3 値を区別できること（(a) を採った場合）
- `tests/headless_seam.rs:89` の `deliver(plan, &mut advisor).unwrap()` が新しい戻り型
  でも green

### 単体テスト
- なし（型と分岐のみ。byte 同一は上記の capture が担う）

## 実装アプローチ
- **Outside-In**: spike で消費者を列挙 -> (c) なら spec 1 文で終了、(a) なら double の
  期待を先に書き換えて赤 -> 型を足す -> 呼び出し側で使う -> spec。実装は
  subagent-driven-development で行う。

## 見積もり
3 SP（E=3日。RICE 4.0）

## 技術的考慮事項
- 依存関係: なし。`src/cli/mod.rs` を PBI 58 / 59 / 60 と共有するので番号順に直列
  （57 -> 58 -> 59 -> 60）
- `direct` は interactive TUI。status が返っても「セッションが終わった」以上の意味は
  無い。exit 0 が advisor の成功を意味しないことは spec に書く
- 過剰設計のサイン: 呼び出し側が `match` せず `let _` にするなら、その型は要らない。
  spike の判定基準にする
- 非機能要件: プライバシー（log path を返す場合も中身は返さない）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `src/cli/mod.rs:417-425` `DeliveryMode`、`:438-447` `DeliveryPayload`、
  `:449-503` `DeliveryPlan`、`:505-520` `AdvisorLaunch`、`:561-614` `deliver`、
  `:606-611` の bail、`:686` の唯一の呼び出し。
- test: `:2818-2890` の 4 本、`tests/headless_seam.rs:89`。
- `docs/specs/cli.md:543-560`（delivery 節）、`:467-475`（optimize の synopsis）。
- 既実装の確認: `deliver` は status を捨てている（`:601-612` で参照するのは失敗判定
  のみ）。戻り型を変える消費者は無い。よって本 PBI は「決める」ことが本体。

### 実装手順
1. spike: 呼び出し側（`optimize`、将来の optimize 拡張）が結果で何をするかを列挙。
   3 案の比較を PR 本文へ
2. (c) を選んだ場合: `docs/specs/cli.md` に 1 文。実装なし。PBI を閉じる
3. (a) を選んだ場合: double の期待を 3 値に更新（赤）-> `DeliveryOutcome` -> `deliver`
   の戻り値 -> `optimize` で 1 箇所使う -> spec 更新
4. 検証: byte 同一の capture、`cargo test`、fmt、clippy

### 落とし穴
- `?` の early return が 3 箇所（`:574`、`:577`、`:601` / `:602`）ある。戻り型を変えると
  それぞれ `Ok(Outcome)` を返す必要がある。(a) ではこの 3 箇所の意図（Print 完了 /
  headless 成功 / direct 成功）を区別する
- `bail!` の message は PBI 52 と 46 の golden が部分的に pin している。文言を変え
  ないこと
- `DeliveryPayload::Full` の早期 return（`:572-576`）は pointer file を作らない。
  `Printed` に log を持たせない
- headless の log path は成功時にも返せる（double が返している）。それを返すと
  `optimize` が「run の log: …」を出せるが、現行の stderr 契約を変えるので (a) の
  範囲では表示を変えない（返すだけ）
- spike を 0.5 日で切る。結論が出ない場合は (c) に倒し、判断材料を spec に残す

## Definition of Done
- [ ] spike の結論が PR 本文と `docs/specs/cli.md` に残っている
- [ ] (a) を採った場合、全BDDシナリオが自動テストとして実装されパスする
- [ ] (c) を採った場合、出力がバイト同一で、再評価の条件が spec に書かれている
- [ ] 失敗 message が不変
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
