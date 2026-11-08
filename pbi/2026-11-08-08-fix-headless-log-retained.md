# PBI: headless 実行の run log 残置を修正する

## ユーザーストーリー
headless で advisor を使う saniwalens ユーザーとして、成功時の advisor
raw 出力の一時ログがディスクに残らず、失敗時にだけ場所が分かるように
したい。なぜなら現状は advisor の生出力（echo された secret を含み得る）が
一時ログに残り続け、パスが stderr に表示される設計だからだ。

## 優先度
- 順位: 08 / 全17件
- RICEスコア: 6.0（Reach=3 / Impact=2 / Confidence=50% / Effort=0.5日）
- 根拠: secret を含み得るファイルを設計上残している状態を解消する。
  Confidence は控えめ（利用実態のばらつき）が、修正コストは小さい。

## BDD受け入れシナリオ
Scenario: 成功時は run log が削除される
  Given  advisor が headless（非 verbose）で正常終了した
  When  実行が完了する
  Then  advisor raw 出力の一時ログ（run log）はディスクに残らない
  And   briefing ファイルと同様に後始末される

Scenario: 失敗時はログが残り場所が通知される
  Given  advisor が headless（非 verbose）で異常終了した
  When  実行が失敗する
  Then  run log はディスクに残る
  And   ユーザーにログのパスが通知される
  （例: `{binary} exited with {status} — raw output: {path}`）

Scenario: 境界 — KEEP opt-in があれば成功時もログを残す
  Given  ログ保持の opt-in（KEEP 系の明示指定）がある
  When  advisor が headless で正常終了する
  Then  run log はディスクに残る
  And   その振る舞いがドキュメントに記載されている

## 受け入れ基準
- [x] 成功時（headless、非 verbose）に run log が削除される
- [x] 失敗時は run log が残り、パスが通知される（現状の bail メッセージを維持）
- [x] 成功時にも残す opt-in（KEEP 系）の有無を決めて固定する。
  ある場合は環境変数・フラグ名と振る舞いをドキュメントに記載する
  → `SANIWALENS_KEEP_RUN_LOG=1`（`SANIWALENS_KEEP_SCRATCH=1` に倣う）。
  失敗時は opt-in なしでも残る。`docs/specs/cli.md`・`docs/specs/harness.md` に記載
- [x] briefing ファイルの後始末（現状の `remove_file(&path)`）が退行しない
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green
  → 本PBI範囲は green（`cargo test headless` 17件+関連パス、`cargo fmt --check` は
  本PBIファイル clean、`cargo clippy` に新規警告なし）。共有ツリー上の他作業
  （`src/core/findings.rs` の fmt 差分・`load_mode` 未使用警告）が残るため、
  ツリー全体の green 確認は親エージェント側で実施すること

## テスト戦略
- 統合: headless 成功パスで run log が削除されること。失敗パスで run log が
  残り、エラーメッセージにパスが含まれること。briefing ファイルの削除が
  維持されること
- 単体: 成功・失敗・KEEP opt-in の分岐条件（ログ削除 vs 保持＋通知）の
  真偽表。パス通知メッセージの形式
- E2E: 実 advisor バイナリ不要の範囲で、合成 advisor による
  headless 成功・失敗の往復

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状: src/cli/headless.rs:64-71 で advisor の raw 出力を一時ログに
  永続化し（`advisor running headlessly — raw progress: {path}` を stderr に
  表示）、headless.rs:316 で `writeln!(run.log(), "{line}")` と毎行追記する。
  呼び出し側 src/cli/mod.rs:616-624 は briefing ファイルだけを
  `fs::remove_file(&path)`（624 行目）で消し、run log を成功時も失敗時も
  消さない。失敗時は 633-639 行目の bail でパスを通知するのみ
- 修正方向: 成功時は run log を削除する。失敗時は残して通知する
  （現状の bail メッセージを維持）。成功時にも残す KEEP opt-in の有無は
  実装時に決め、ある場合は名前と振る舞いをドキュメント化する
- 削除は成功パス・失敗パス・spawn 失敗（`run_log` が `None`）の全分岐で
  見直す。`remove_file` のエラーを無視する現行規約（624 行目の `let _ =`）
  に合わせるかも実装時に決める
- プライバシー規約: 実 transcript・実 config・生成 DB をコミットしない。
  テスト fixture は合成のみ（session-data-privacy 規約）
- エビデンス: src/cli/headless.rs（64-71 行目のログ作成と stderr 表示、
  316 行目の毎行追記）、src/cli/mod.rs（616-624 行目の headless 分岐と
  briefing のみの削除、632-640 行目の失敗時 bail）
- 関連コード: src/cli/headless.rs の `write_private_file` / `log_path` 周り
  （private file 作成の既定）、失敗時メッセージの capture テスト

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）（親エージェント側で実施）
- [x] ドキュメント更新済み（`docs/specs/cli.md`・`docs/specs/harness.md`）

## 実装メモ（2026-11-08）
- 削除の所有権は呼び出し側 `deliver` ではなく `run_headless` に置いた。
  `HeadlessRun` がログの lifetime を所有しているため。`deliver` の失敗時
  bail（`{binary} exited with {status} — raw output: {path}`）は無変更。
- `run_headless(..., keep_log: bool)` に引数追加。`ProcessAdvisor` が
  `keep_run_log()`（`SANIWALENS_KEEP_RUN_LOG` 読取）を渡す。純粋述語
  `should_remove_run_log(success, keep)`（= `success && !keep`）を真偽表テスト。
  env をテストで触らない（`SANIWALENS_KEEP_SCRATCH` の並列テストの教訓）。
- 成功＋非 readable でも削除するが、その場合の stderr 注意書きはパス無し
  文面（`run log removed`）に切替。保持時のみパス付き文面。削除済みパスを
  指す表示の dangling を避けるため。
- ついで修正（本PBI由来の不安定化の解消）：並列テストが同一 tick で
  `private_temp_path` を衝突させ `AlreadyExists` で落とす既存の弱点を、
  プロセス内 atomic sequence 付与で解消（`src/cli/mod.rs`）。パス形状に
  依存する既存テストなし。
- 新規テスト（`cargo test headless` で取得）: 真偽表、成功時削除、KEEP 時保持、
  失敗時保持（`run_headless` 層）、`deliver` 層の失敗時保持＋パス通知、
  headless briefing 削除。いずれも合成 fixture（`/bin/sh` 合成 advisor・
  一時ファイル）のみ。
