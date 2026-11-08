# PBI C10: 秘密temp RAII + 既定timeout

- 種別: fix / Priority rank: 4 / RICE: 3.2 (Reach 4 / Impact 2 / Confidence 80% / Effort 2)
- 依存: C9 の後 (同ファイル `src/cli/mod.rs`, `src/cli/headless.rs` を触るため)
- 見積: 2

## ユーザーストーリー

CLI利用者として、briefing用秘密tempファイルがクラッシュ/タイムアウト後も残らず、advisorハング時も既定時間で打ち切られてほしい。残留による秘密漏えいと無限待ちをなくすため。

## 背景 (file:line)

- 生成: `src/cli/mod.rs:814-825` (`create_new` + `0600`) に対し、破棄は `:677` の `let _ = remove_file` のみ (エラーパスで漏れる)。
- 同一idiom: `src/cli/headless.rs:285` (run-log), `src/cli/views/guide.rs:407`。
- 既存guardの手本: `HeadlessRun` (`src/cli/headless.rs:36-49`, `174-190`) は `Drop` で後片付けする。
- 待機パス: `src/cli/headless.rs:140-171` は `deadline None` で永久ブロック、`381-398` の `recv` もブロック。`mod.rs:669-676` の `timeout: Option` は既定 `none`。`mod.rs:119-121,347` の `abandon_reader` はdetachするだけで回収しない。

## BDD受け入れシナリオ

```gherkin
Scenario: クラッシュ/タイムアウトしてもbriefingファイルが残らない
  Given briefing用tempファイルが作成済み
  When advisor実行がpanic/timeout/中断で終了する
  Then RAII guardのDropによりtempファイルが削除される
  And 同一ディレクトリに残留ファイルがない

Scenario: ハングしたadvisorは既定timeoutで打ち切られる
  Given timeout未指定でadvisorが応答しない
  When 既定timeoutが経過する
  Then 待機はエラー終了しtempはRAIIで削除される
  And プロセス・readerスレッドがゾンビ化しない
```

## 受け入れ基準

- [ ] briefing tempの削除が `Drop` ベース (RAII guard) になり、正常/異常系の全パスで残留しない
- [ ] `timeout: Option` の既定値が有限 (例: 60s) になり、`None` の無限待ちが明示指定時のみ
- [ ] `headless.rs:140-171,381-398` の待機がdeadline付きになる
- [ ] `abandon_reader` パスでもguardが有効、またはdetach後の回収方針が明記される
- [ ] 既存テスト (`mod.rs` / `headless.rs` 関連) が全て通る
- [ ] C9とコンフリクトしない (C9適用後にrebase/適用)

## テスト戦略 (t-wada, Outside-In)

- E2E (失敗から): advisorをハングさせた結合実行で、既定timeoutで終了+temp残留なしを確認
- 統合: timeout `None` 指定時の既定値解決テスト (plumbing test: `None` → 既定値、明示値 → 優先)
- 単体 (多数): guardのdrop test (`TempGuard::drop` で `remove_file` 呼ばれること、panic時も含む)、deadline計算の境界値テスト

実装順: guard単体 (Red→Green) → timeout plumbing → 結合/E2Eで全体確認 → リファクタ。

## 実装者向け注記

- 手本: `HeadlessRun` (`headless.rs:36-49,174-190`) と同じ `Drop` パターンで `BriefingTempGuard { path }` を作り、`mod.rs:814-825` の生成直後に保持させる。`:677` の手動 `remove_file` はguardに寄せる。
- `headless.rs:285`, `guide.rs:407` は本PBIでは直さない (同一idiomとして将来展開、スコープ外と明記)。
- `mod.rs:669-676` の `timeout: Option<Duration>` に `unwrap_or(DEFAULT_BRIEFING_TIMEOUT)` を一箇所で噛ませ、呼び出し側に分散させない。
- 落とし穴: `abandon_reader (:119-121,347)` でguardを先にdropしないこと (readerがpathを読む間はguardを生かす)。C9と同一箇所を編集するため必ずC9の後に作業する。

## Definition of Done

- [ ] 全BDDシナリオが自動テストとしてパスする
- [ ] guard drop test + timeout plumbing testが追加されパスする
- [ ] `clippy` / 関連既存テストが通る
- [ ] 残留temp・無限待ちの再発チェック手順が本ファイルに記載済み

## 検証記録 (B2統合・2026-11-09)

- 実装: BriefingFile RAIIガード (`src/cli/mod.rs`、Dropで除去＋失敗時stderr警告、NotFound許容)、`DEFAULT_BRIEFING_TIMEOUT=60s`＋`resolve_timeout` (`src/cli/headless.rs`、0=無限+警告)。
- 残temp/無限waitの再確認手順: `SANIWALENS_KEEP_RUN_LOG=1` 付きheadless失敗でrun logが残ること、kill -9後の/tmp残置を次回起動で警告すること、`--timeout 0` で警告が出て無制限になることを目視確認する（手動QA・ユーザー作業として残す）。
- 境界検証: fmt OK、clippy 0 errors、lib 640 passed、`briefing_raii` 5 passed（panic-leakはRed→Green確認済み）、`headless_seam` 2 passed、line-budget内 (mod.rs 2960/3037)。
- ついでの統合修正: Drop内のcollapsible-ifをlet-chain化（clippy指摘対応、振る舞い同一）。
