# PBI: windowed view の列挙を clap から導出し help capture の集合一致を pin する

## ユーザーストーリー
保守担当者として、windowed view の一覧が 3 箇所の手書き配列ではなく clap の
subcommand 木から導出されてほしい、なぜなら view を追加しても既存要素が不変なら
help capture も conflict テストも静かに通り、次の 1 回で盲点になるからだ。

## 優先度
- 順位: 7 / 全候補数 10
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1.0日）
- 根拠: 現在の欠陥ではないが、次の view 追加で無検証の help/conflict が出荷される。
  ④（WindowedRead）の後に置き、`src/cli/window.rs` の変更を直列化する。

## ビジネス価値
- view 追加時に「capture を足す」「conflict を確認する」が構造的に強制される。
- zip の短い側で黙る問題が消え、PINNED の名前集合と clap 導出集合が一致する。
- 測定方法: PINNED から 1 行消すと赤、合成 enum に 10 個目の windowed view を足すと赤。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 4本）
- 手書き列挙は 3 箇所:
  `tests/cli_help_golden.rs:9-19` の VIEWS [9]、
  `src/cli/window.rs:425-434` の SINGLE_WINDOW_VIEWS [8]、
  `src/cli/mod.rs:3002-3012` の views [9]（候補文に無い 3 箇所目）。
- 各 assert は「要素の性質」だけを検査し、「集合の完全性」を主張しない。
  `tests/cli_help_golden.rs:194` の zip は短い側で黙る。
- 合成 enum に 10 個目の windowed variant（Cleanup）を差し込むと、現行テストは
  9/9 が解決し続けて GREEN。推奨 tripwire は exit 1。
- 根本原因: 「windowed view とは何か」の定義が実装（clap の subcommand 木）から
  導出されず、テスト側の 3 つの手書き配列に複製されている。

## BDD受け入れシナリオ

```gherkin
Scenario: help capture の集合は clap の windowed view と一致する
  Given clap の subcommand 木から --last を持つ view を導出する
  When PINNED の名前集合と比較する
  Then 過不足が 0 で、VIEWS と HELP は 1 つの表に畳まれている

Scenario: view を追加すると tripwire が赤になる
  Given 合成 enum に 10 個目の windowed view を足す
  When help golden の tripwire を実行する
  Then "missing from the hand list" で失敗する

Scenario: window.rs と mod.rs の列挙が消える
  Given SINGLE_WINDOW_VIEWS と mod.rs の views を削除する
  When conflict / period のテストを導出に置き換える
  Then 9 view（trend 含む）で同じ assert が green になる
```

## 受け入れ基準
- [ ] `tests/cli_help_golden.rs` の VIEWS + HELP を `PINNED: [(&str, &str); 9]` の
  1 表に畳み、zip を削除する
- [ ] `windowed_views()` を `Cli::command().get_subcommands()` から導出する。
  `--last` を持つ subcommand を windowed と判定し、`build()` は呼ばない
  （build すると help subcommand が混入する）
- [ ] PINNED の名前集合と導出集合の一致、および件数 9 を assert する。
  失敗メッセージに missing / stale と `cli-view-addition.md` への参照を含める
- [ ] `src/cli/window.rs` の `SINGLE_WINDOW_VIEWS` を削除し、テスト内で導出する。
  trend も loop に入れ、`windowed_flags` の Trend アームを使う
- [ ] `src/cli/mod.rs:3002-3012` の 3 つ目の配列を削除し、同じ導出に置き換える。
  mod.rs は約 8 行縮むため、同一 PR で budget を実測して下げる（zero-slack）
- [ ] 失敗メッセージを定数化し、文面自体を単体 assert する
- [ ] 任意: `WHOLE_HISTORY = ["analyze", "sql", "optimize"]` との partition assert を
  足し、windowed と whole-history が全 subcommand を覆うことを pin する
- [ ] 契約更新: `.claude/rules/cli-view-addition.md` に capture 1 行 + tripwire、
  `docs/specs/cli.md:74` / `:181` / `:209` の nine/eight を数え直すか数を使わない文にする
- [ ] `cargo test --test cli_help_golden` と `cargo test --lib cli::window` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 9 view の `--help` が PINNED の capture と byte 一致

### 統合テスト
- `windowed_views()` の集合 == PINNED の名前集合
- PINNED から 1 行消すと赤（変異確認）
- 合成 enum で 10 個目の view を足すと赤（probe P2 相当）

### 単体テスト
- 失敗メッセージが missing / stale と rule 参照を含む
- window.rs の conflict テストが trend を含む 9 view で green

## 実装アプローチ
- **Outside-In**: tripwire を先に書き、PINNED から 1 行消して赤を確認 ->
  表化 -> window.rs / mod.rs の列挙削除 -> budget 再測定 -> 契約更新。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
1 SP（E=1.0日。RICE 12.0）

## 技術的考慮事項
- 依存関係: ④ -> ⑩（`src/cli/window.rs`）、⑤ は trend アームで隣接
- integration test から `saniwalens::cli::Cli` + `clap::CommandFactory` が使えるか
  最初に確認する。使えなければ `[dev-dependencies]` に clap の同版を足す
- `Cli::command()` は `build()` しない。`build()` は help subcommand を足す
- alias / hidden は現状 0 件で契約外。判別子 `--last` が将来崩れる可能性に備え、
  件数 9 の assert を併置する
- mod.rs の budget は実測 `wc -l` に合わせ、spec 更新と同一 PR で行う

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `tests/cli_help_golden.rs:9-19` VIEWS、:21-30 HELP、:194 zip。
- `src/cli/window.rs:425-434` SINGLE_WINDOW_VIEWS、:525 の loop、
  :448-455 の Trend アーム。
- `src/cli/mod.rs:3002-3012` の views、:3033-3099 の trend conflict pin。
- `tests/cli_mod_line_budget.rs:19` の budget（現在 3,100、slack 0）。
- `.claude/rules/cli-view-addition.md` の acceptance template。

### 実装手順
1. 赤: PINNED から 1 行消す / 合成 enum で tripwire が落ちることを確認
2. PINNED の表化と zip 削除
3. `windowed_views()` の導出と集合一致 assert
4. window.rs の列挙削除 + 導出
5. mod.rs の 3 つ目の削除 + budget 再測定
6. rule と cli.md の文言更新
7. 検証（cli_help_golden、window、budget、フルスイート）

### 落とし穴
- `Cli::command().build()` を呼ぶと help subcommand が増える。`get_subcommands()` を
  そのまま使う
- `--last` を持たない windowed view が将来現れると静かに外れる。件数 assert を併置
- PINNED の名前と capture の順序は現行の capture 順を保つ
- budget を下げ忘れると slack が残る。zero-slack 規範を守る

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 3 つの手書き列挙が 0 になり、clap 導出 + PINNED 1 表になっている
- [ ] PINNED から 1 行消す / 合成 view 追加で赤
- [ ] budget が実測値に更新されている
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] cli-view-addition.md と cli.md 更新済み
