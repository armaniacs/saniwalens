# PBI: optimize と --apply のプロローグ処理を 1 関数に切り出す

## 優先度
- 順位: 12 / 全14件
- RICEスコア: 2.0（Reach=5 / Impact=1 / Confidence=80% / Effort=2日）
- 根拠: run_apply は handoff.rs の docstring（src/cli/handoff.rs:4-5）で
  「配信（delivery）をバイト同一に保つため optimize と別経路」と宣言しているが、
  配信以前の準備処理（analyze 更新＋通知行、db_display の canonicalize、
  HarnessCopy への言語注入、briefing→一時ファイル→launch_prompt 組立）が
  cli/mod.rs の optimize とほぼ逐語コピー。「store: refreshed just now」の文面は
  実 3 箇所に存在し、文言や canonicalize フォールバックの変更時に 2-3 箇所の
  同時修正が必要。金銭経路（advisor 起動）の挙動差異につながる箇所だけに、
  ドリフト時の影響は大きい

## ユーザーストーリー
optimize と --apply の両経路をメンテする開発者として、配信以前の準備処理
（プロローグ）が 1 関数に集約され、片側だけの修正で金銭経路が黙ってドリフト
しない状態がほしい。なぜなら --apply は advisor を実際に起動する経路であり、
重複のドリフトは optimize 側の出力からは見えない形で apply の挙動差異として
表面化するからだ。

## 背景
- 重複箇所（現行の行番号で確認済み）:
  - run_analyze 呼び出し＋通知行: `src/cli/mod.rs:585-596` vs
    `src/cli/handoff.rs:289-300`。同じ文言の第 3 の実装が
    `src/session.rs:459-467`（`Session::refresh`、private、fn 全体は 450-470）
  - db_display の canonicalize＋フォールバック: `src/cli/mod.rs:602-604` vs
    `src/cli/handoff.rs:306-308`
  - HarnessCopy への言語注入: `src/cli/mod.rs:610-611` vs
    `src/cli/handoff.rs:313-314`
  - briefing→一時ファイル→launch_prompt の組立: `src/cli/mod.rs:630-644` vs
    `src/cli/handoff.rs:316-325`。mod.rs 側は render_briefing の結果を
    DeliveryPlan::launch に渡し（一時ファイル化は deliver 側）、handoff.rs 側は
    write_private_tempfile / BriefingFile から inline で組む
- 通知行「store: refreshed just now (… re-analyzed, … unchanged)」の文言が
  実 3 箇所（src/cli/mod.rs:590 / src/cli/handoff.rs:294 / src/session.rs:462）。
  findings 収集（collect_findings(&sess.store, TimeRange::ALL)）も
  src/cli/mod.rs:598 と src/cli/handoff.rs:302 で同一
- 分離の宣言は配信のみを射程にする: src/cli/handoff.rs:4-5 の docstring は
  「Normal `optimize` never enters this module, so its delivery stays
  byte-identical…」と述べる。別経路が必要なのは配信以降で、配信以前の
  プロローグに分離する必然はない
- 改善案: session.rs（または deliver.rs 隣接）に
  prepare_optimize(sess, projects, frozen) ->
  Result<(Findings, String /*db_display*/, HarnessCopy)>
  相当の 1 関数を切り出し、refresh の通知行もそこに集約する。
  Session::refresh を pub(crate) 化して再利用するのが最小
- 配信部分（DeliveryPlan / capture_headless_log）は現状の分離を維持 —
  optimize と --apply の出力（stdout/stderr、生成ファイル内容）は
  バイト同一を保つ
- 前提・順序: 同日の PBI 11（apply-plan-byte-cap、plan 上限）が handoff.rs に
  先行着地する前提。11 の後に着手（台帳 pbi/2026-11-21-00-backlog-holistic.md
  の処理順 11 → 12 → 14、依存欄は 07（session.rs）・11（handoff.rs）の後）。
  session.rs への変更は最小（pub(crate) 化）に留める
  （PBI 13 の session.rs 責務分割は後続であり、本 PBI では先走らない）

## BDD受け入れシナリオ

```gherkin
Scenario: リファクタ前後で optimize と --apply の出力はバイト同一である
  Given 同じ合成 store と transcripts がある
  When プロローグを 1 関数に切り出すリファクタの前後で optimize（--apply なし）と
    --apply を同じ入力で実行する
  Then 両コマンドの stdout・stderr がリファクタ前とバイト同一である
  And 生成される briefing・plan ファイルの内容もバイト同一である

Scenario: プロローグの実装は 1 箇所に集約される
  Given 切り出された prepare_optimize 相当の 1 関数がある
  When ソース内で通知行の文言を検索する
  Then 「store: refreshed just now (… re-analyzed, … unchanged)」は
    実装中 1 箇所にしか現れない
  And db_display の canonicalize と HarnessCopy への言語注入の実装も 1 箇所である
  And optimize・--apply・analyze の各経路が同じ関数を通って同じ文面を出す

Scenario: frozen のときはプロローグの分析も通知行も走らない
  Given --frozen を付けて optimize と --apply を実行する
  When プロローグ処理が走る
  Then run_analyze と通知行の stderr 出力は現行どおり行われない
  And canonicalize・言語注入・findings 収集・briefing 組立は現行どおり行われる
```

## 受け入れ基準
- [x] プロローグ（analyze 更新＋通知行、findings 収集、db_display の
  canonicalize、HarnessCopy への言語注入）が prepare_optimize 相当の
  1 関数に集約される
- [x] 「store: refreshed just now」の文言がソース内 1 箇所になり、
  src/cli/mod.rs / src/cli/handoff.rs / src/session.rs の重複実装が消える
- [x] Session::refresh への変更は pub(crate) 化など最小に留まり、
  session.rs の責務分割（PBI 13）の内容を先行しない
- [x] 配信部分（DeliveryPlan / capture_headless_log）の分離が維持され、
  optimize と --apply の出力（stdout/stderr、briefing・plan の生成ファイル内容）
  がバイト同一である（リファクタ前後の capture diff で確認）
- [x] frozen 指定時に analyze も通知行も走らない現行挙動が保たれる
- [x] PBI 11（apply-plan-byte-cap）着地後の handoff.rs を起点に実装されている
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- E2E: 既存 optimize / --apply の E2E・golden が等価性の証人（出力バイト同一）。
  リファクタ前後の capture diff で stdout/stderr と生成ファイル内容を確認する
- 統合: frozen / 非 frozen の両モードで optimize と --apply を通し、
  通知行の出る・出らないが現行と同じであることを確認する
- 単体: 切り出した prepare 関数の戻り値（Findings・db_display・
  HarnessCopy の language）を固定する。canonicalize 失敗時のフォールバック
  （元パスのまま使う）もここで検証する

## 見積もり
2日（RICE の Effort と同一根拠）。切り出し自体は機械的だが、
バイト同一性の全経路検証（optimize / --apply × frozen / 非 frozen）と
session.rs への変更を最小に保つ作業が実質の分量

## 実装者向け注記
- 切り出し先は session.rs（または deliver.rs 隣接）。
  Session::refresh（src/session.rs:450-470）を pub(crate) 化して再利用するのが
  最小で、通知行は prepare 側に集約する
- --apply 固有の前段は切り出さない: src/cli/handoff.rs:284-288 の
  launch_command / require_headless_mode / plans_dir はプロローグではなく
  --apply 固有の処理（配信側）に属する
- briefing 組立の差異に注意: mod.rs は render_briefing の文字列を
  DeliveryPlan::launch に渡し一時ファイル化は deliver 側、handoff.rs は
  write_private_tempfile → BriefingFile → launch_prompt を inline で組む
  （src/cli/handoff.rs:316-325）。配信の分離を壊さない範囲でのみ共通化し、
  共通化しない場合は現状の分離を維持する
- spec 同期: docs/specs/cli.md の optimize（463 行目付近）と
  `--apply`（597 行目付近）の節に prepare 関数の記述を足す要否は実装時に判断
  （.claude/rules/spec-sync.md に従い同梱更新する）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test がすべて green
- [x] optimize・--apply の出力バイト同一性を確認済み（リファクタ前後の diff 空）
- [x] コードレビュー完了（spec 準拠 + コード品質）
- [x] ドキュメント更新済み（docs/specs/cli.md との同期を含む）
