# PBI (スパイク): 対話起動形状の確定 (optimize 適用ハンドオフ先行調査)

本体 (`pbi/2026-11-09-01-feat-optimize-apply-handoff.md`) の先行作業。本体より先に実施すること (番号は前後するが実施順は本ファイルが先)。

## ユーザーストーリー
本体 PBI の実装者として、各 harness の対話セッションにファイル渡しで起動できる形状を確定したい。なぜなら渡し口の有無で本体設計 (起動経路とフォールバック) が変わるからだ。

## ビジネス価値
- 本体の手戻りをなくす (不確実性の分離)。測定方法: 下表が埋まり本体の起動経路が一意に決まること

## BDD受け入れシナリオ

Scenario: 各 harness の渡し口が表になる
  Given 調査対象の3 harness がある
  When  対話起動の形状を1つずつ確認する
  Then  下表の起動形状と可否がすべて埋まっている

## 受け入れ基準
- [x] 下表の3行がすべて埋まっている (コマンド、渡し方、確認方法)
- [x] opencode 行の結論が出ている (渡し口あり/なし。なしの場合は本体でのフォールバック案つき)
- [x] 結果が本体 PBI (`01-feat`) に反映されている (起動経路の決定またはフォールバック採用)
- [x] 実コードの変更はなし (調査のみ。試行錯誤の産物はコミットしない)

## 調査対象の表

| harness | 対話起動の候補 | ファイル渡しの可否 | 確認方法 |
|---------|---------------|-------------------|---------|
| claude | `claude` に対話起動 (既存の Interactive 経路) | 可のはず。ポインタプロンプト流用可か | 既存コード読みで確定 |
| kilocode | `kilo --prompt` (既存の Interactive 経路) | 可のはず。ファイル読み指示の同梱可か | 既存コード読みで確定 |
| opencode | `opencode` TUI への起動時プロンプト/ファイル指定 | 未確認。要実機確認 | 実機で起動して確認する |

## テスト戦略 (t_wadaスタイル)
- スパイクのため自動テストなし。代わりに上表の確認方法欄に実行コマンドと結果を残す
- 実機確認は合成データのみで行い、実 transcripts / 実 config を触らない

## 見積もり
0.5 (要チームでの見積もり。調査のみ、コード変更なし)

## 技術的考慮事項
- 依存関係: 本体 (`01-feat`) は本スパイクの完了が前提。逆依存なし
- 着眼点: `Harness::launch_command` (`src/adapter/harness.rs`) の周辺と `cli/deliver.rs` の受け口
- opencode TUI に渡し口がない場合の候補: 手動コマンド表示フォールバック、または TUI 内でのファイル読み指示文の表示。本体設計に影響するため結論にフォールバック案を含める

## 実装者向け注記

### 現状コードの確認
```bash
grep -n "launch_command" -A 20 src/adapter/harness.rs | head -n 40
```

### 落とし穴
- opencode の `run` はヘッドレス専用と混同しないこと。調べるのは対話 TUI (`opencode` 無引数相当) への渡し方
- 実機確認で実プロジェクトの transcripts を読ませないこと。合成の空 home で十分

## Definition of Done
- [x] 上表がすべて埋まっている
- [x] opencode 行に結論とフォールバック案がある (結論: 渡し口ありのためフォールバック不要)
- [x] 本体 PBI (`01-feat`) の起動経路に関する記述が更新されている (Task 2 の範囲)

## 調査結果 (Task 1 スパイク、実施日 2026-11-09)

### 完成した表

| harness | 対話起動の候補 | ファイル渡しの可否 | 確認方法 |
|---------|---------------|-------------------|---------|
| claude | `claude "<prompt>"` (末尾 positional 引数、`RunMode::Interactive`) | 可。`launch_prompt` のポインタ文 (briefing ファイルパス埋め込み「Read that file in full ...」) を `deliver.direct` が argv 末尾に載せる。briefing 本体は `0600` temp ファイル経由 | コード読みで確定: `src/adapter/harness.rs:589-591`、`src/cli/deliver.rs:145-155`、`src/cli/mod.rs:563-594`、`src/core/optimize.rs:643-655`。`claude --help` (合成 HOME) で `Usage: claude [options] [command] [prompt]` / interactive default を確認 |
| kilocode | `kilo --prompt "<prompt>"` (`RunMode::Interactive`)。bare `kilo <text>` は不可 (ファイルオープン扱い) | 可。claude と同一ポインタ文方式 | コード読み + 実機 help で確定: `src/adapter/harness.rs:597-605`、`docs/specs/harness.md:175-182`、`kilo --help` (合成 HOME) に `kilo [project]  start kilo tui  [default]` + `--prompt  prompt to use  [string]` あり |
| opencode | `opencode --prompt "<prompt>"` (TUI root の flag。`run` サブコマンドとは別物) | 可 (渡し口あり)。同一ポインタ文を `--prompt` 値に載せれば briefing ファイル渡しが成立。positional は `<directory>` のため使用不可 | 実機で確認 (合成 HOME のみ、opencode v2.2.6): `opencode --help` に `USAGE opencode <subcommand> [flags] [<directory>]` + `FLAGS --prompt string  Prompt to use` あり。`opencode run --help` は別物 (`run [flags] [<message...>]`) と確認。`timeout 8 opencode --prompt "spike-probe-prompt" </dev/null` は flag エラーなく起動 (EXIT=0)。TUI 全画面の目視は非 TTY のため未実施 |

### 結論

- opencode TUI への渡し口あり (`opencode --prompt`)。フォールバック案 (手動コマンド表示) は不要。TTY のない環境では既存 `--print` 経路が代替になる (新規実装不要)。
- 本体への申し送り (起動経路の決定): claude `("claude", &[])` / kilo `("kilo", &["--prompt"])` は現行どおり。opencode 対話形は新規形状 `("opencode", &["--prompt"])` + `RunMode::Interactive` (現行 `run ...` Headless とは別)。`direct` は `leading` の後に prompt を足すため `leading = &["--prompt"]` で `opencode --prompt "<pointer>"` になる (kilo と同型)。`deliver` 側は mode 駆動のため変更不要の見込み。本体実装時に TTY 付き目視 1 回を推奨。
- 詳細は `.superpowers/sdd/task-1-report.md` を参照。実データ不使用 (全コマンド合成 HOME)。`src/` / `tests/` 無変更。
