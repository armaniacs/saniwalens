# PBI: briefing 上限 (候補 C9)

- Priority: rank 1
- RICE: 8.0 (Reach 8 / Impact 3 / Confidence 100% / Effort 3)
- Estimate: 3
- 種別: fix

## 背景

`optimize` / `doctor` 系の briefing 組み立てが、マルチ年次規模の store に対して無制限に読み出す。

- `src/cli/mod.rs:838-881`: `findings_input` が 12 件の uncapped な store 読み出しを行う
- `src/store.rs:1073` (`file_edits`)、`:1121` (`work_counts`)、`:1236` (`error_tool_breakdown`)、`:1278` (`skill_usage`)、`:1546` (`subagent_by_agent`)、`:1586` (`skill_event_costs`)、`:1707` (`effective_catalog`)、`:1750` (`session_roots`)
- `src/core/findings.rs:223,227`: `take(N)` は描画 head の切り詰めのみで、store 読み出し自体は制限しない
- `src/core/optimize.rs:425-460`: 1 excerpt あたり 300 文字 cap のみで、全体バイト上限がない
- `src/cli/mod.rs:875-879`: `skill_out_tokens` を `Vec` で全件取得後に `src/core/findings.rs:336` で `sum` している

## ユーザーストーリー

マルチ年次 store を持つアナリストとして、`optimize` / `doctor` を実行したい。store が大きくても briefing が有界に終わり、メモリ・遅延が爆発しないようにしたい。

## BDD シナリオ

### Scenario 1: 大規模 store でも briefing は有界に終わる

- Given マルチ年次規模の store が存在する
- When `optimize` / `doctor` の briefing を組み立てる
- Then 全体の briefing バイト数・行数・読み出し件数に上限が効き、無制限の全件取得を行わない

### Scenario 2: skill_out_tokens は SUM クエリで集計する

- Given `skill_out_tokens` の集計が必要である
- When briefing 用の使用量を集計する
- Then `Vec` 全件取得 + アプリ側 `sum` ではなく、SQL の `SUM` クエリで集計する

### Scenario 3: BRIEFING_MAX_BYTES 超過は切り詰めマーカーを残す

- Given briefing が `BRIEFING_MAX_BYTES` を超える
- When briefing 本文を確定する
- Then 末尾を切り詰め、切り詰めマーカー (truncated 標識) を付与する

## 受け入れ条件

1. `findings_input` 経路の store 読み出しに `LIMIT` / 上限が付くこと。
2. `skill_out_tokens` が SQL `SUM` 集計になり、全件 `Vec` 取得をしないこと。
3. briefing 全体に `BRIEFING_MAX_BYTES` 上限が効くこと。
4. 上限超過時は切り詰めマーカーが残り、黙って欠落しないこと。
5. `take(N)` の描画切り詰めと store 側の制限の責務が混同されないこと。
6. 大規模 fixture でも briefing のメモリ・時間が有界であること。

## テスト戦略 (t-wada)

- pure core の cap ロジック: 小さな synthetic fixture による単体テスト (上限・切り詰めマーカー)。
- store の `LIMIT` / `SUM` テスト: 大規模データを模した fixture で件数・合計値の正当性を検証。
- golden テスト: 上限超過時の briefing 出力 (マーカー付き) のスナップショット。

## 実装メモ

- 上限値は定数 (`BRIEFING_MAX_BYTES` 等) に集約し、core と CLI で二重定義しない。
- `SUM` 化するクエリと `LIMIT` 化する読み出しの一覧を PR 説明に明記する。

## DoD

- `cargo fmt` / `clippy` / `test` が green であること。
- 契約変更がある場合 `docs/specs/cli.md` または `storage.md` を同一変更で spec-sync すること。

## 検証記録 (B1統合・2026-11-09)

- 実装: bounded `*_briefing` readers + `skill_out_tokens_sum` (`src/store.rs`)、fetch caps (`src/core/findings.rs`)、`BRIEFING_MAX_BYTES`切詰め (`src/core/optimize.rs`)、`findings_input`切替え+SUM配線 (`src/cli/mod.rs`)、spec-sync (`docs/specs/cli.md`)。
- 境界検証: `cargo fmt --check` OK、`cargo clippy --all-targets --all-features -- -D warnings` 0 errors、lib 636 passed、新規 `briefing_bounds` 10 passed。受入6件すべて満たす（#2は `vec![SUM]` 配線で全件Vec取得なし）。
