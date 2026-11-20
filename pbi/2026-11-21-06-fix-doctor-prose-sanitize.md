# PBI: doctor の prose 出力を sanitize チョークポイントに揃える

## 優先度
- 順位: 06 / 全14件
- RICEスコア: 16.0（Reach=4 / Impact=2 / Confidence=100% / Effort=0.5日）
- 根拠: `render()` が唯一のターミナル・チョークポイントという invariant
  （src/cli/report.rs:260-263 付近）に対し、doctor の prose 行は生 `println!`
  でバイパスしている。トリガは敵対的/幻覚的な `file_path` を含むセッション
  1 つだけで、ファイル実在は不要。2026-11-16 の vulnhunt がこのクラスを
  誤トリアージした訂正として新規。

## ユーザーストーリー
doctor の「WHAT TO FIX FIRST」を読んで直す場所を決める saniwalens ユーザーとして、
transcript 由来の `file_path` や project 名が制御文字を含んでいても端末にそのまま
流れない状態がほしい。なぜなら敵対的または幻覚的な `file_path` 1 つで ESC/C1 列が
prose に混入すると、出力の読み取りを妨害され、全出力が `render()` の sanitize を
通るという invariant が崩れるからだ。

## 背景
- invariant: `sanitize_cell` の doc comment が「`render()` is the single choke
  point where they would reach the terminal」と宣言する（src/cli/report.rs:260-263
  付近、本体は src/cli/report.rs:270）。Table / Markdown 経路はここで sanitize 済み
- バイパス: doctor は prose を生 `println!` で出力し、補間値が sanitize を通らない
  - src/cli/views/doctor.rs:108-111 付近 — `display(p)`（`p.project` または
    `tilde_path(&p.root)`）の生補間
  - src/cli/views/doctor.rs:124-131 付近 — `worst.file`（thrash 再編集行）の生補間
  - src/cli/views/doctor.rs:132-139 付近 — `p.project` を
    `saniwalens failures --scope project:{}` に生補間
- 正規化漏れの一次原因: `ThrashEpisode.file()` は transcript の Edit/Write
  `file_path` の basename を無切り捨て・無サニタイズで返す。work events の抽出
  （src/adapter/transcript.rs:599-609 付近の `extract_work_events`）は生のまま push
  する。opencode 側は whitespace-collapse + 200 字 truncate が正例
  （src/adapter/opencode.rs:685-705 付近）で、tool_error 経路も既に
  `clean_truncate` を通る（src/adapter/transcript.rs:516-523 付近）— work events
  だけが正規化から漏れている
- したがって ESC/C1 列を含む tool 入力が doctor の prose にそのまま流れる

## BDD受け入れシナリオ

```gherkin
Scenario: ESC/C1 を含む file_path は doctor の prose に制御文字を運ばない
  Given Edit の file_path に ESC と C1 制御列を含む文字列を記録したセッションが 1 つある
  When doctor を実行する
  Then stdout に ESC / CSI / OSC / BEL / C1 の各バイトが含まれない
  And stuck 再編集行は制御文字を除いた basename を表示する

Scenario: 200 字超と空白混じりの file_path は opencode と同じ正規化を受ける
  Given 200 字を超え空白を含む file_path を Edit した work event がある
  When extract_work_events の結果を検査する
  Then id と path は whitespace-collapse と 200 字 truncate 済みである

Scenario: 正常系の doctor 出力はバイト同一である
  Given 制御文字を含まない通常の transcript と findings
  When この修正の前後で doctor を実行する
  Then stdout はバイト同一である（テーブル経由の表示は既に sanitize 済みで不変）
```

## 受け入れ基準
- [x] `extract_work_events` が返す WorkEvent（id / path）が opencode と同じ
  whitespace-collapse + 200 字 truncate を受ける（境界正規化）
- [x] doctor の補間値（`worst.file()` / `p.project` / `tilde_path(&p.root)`）が
  `sanitize_cell` 経由になり、ESC / C1 / BEL が端末に到達しない
- [x] ESC/C1 列を含む `file_path` の合成 fixture で、doctor 出力が制御文字を
  含まないことを確認する新規テストがある
- [x] 既存 doctor 出力（正常系）がバイト同一である（出力の大部分は不変）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: `extract_work_events` の境界テスト。ESC / C1 / BEL を含む `file_path`、
  200 字超、空白混じりの各ケースで opencode の truncate と同じ結果になることを
  固定する。既存の work events テストと core::thrash のテストが壊れないことも確認
- 統合: doctor ビューに合成 findings を渡し、prose 行（108-111 / 124-131 / 132-139
  に相当する行）に制御バイトが混入しないことを assert
- E2E: 正常系の doctor 出力のバイト同一性（既存 golden / capture の diff）。
  テーブル経由の表示は既に安全なので差分ゼロが判定線

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- **ファイル排他（着手条件）**: 本 PBI は src/adapter/transcript.rs に触れる。
  同日の PBI 02（usage 飽和）も transcript.rs を触るため、バッチの実行順で
  **PBI 02 の後に着手する**こと。PBI ファイル自体は作成済みで着手待ち。
  台帳の依存行も同じ結線（pbi/2026-11-21-00-backlog-holistic.md の「02 → 06
  （transcript.rs）」）
- 修正方向は2層:
  (1) 境界正規化 — `extract_work_events` 側で opencode と同じ whitespace-collapse
  + 200 字 truncate を適用する（src/adapter/opencode.rs:698-705 付近の `truncate`
  が正例。src/adapter/transcript.rs:516-523 の `clean_truncate` と同型）
  (2) 出力防御 — doctor の補間値を `sanitize_cell` 経由にする。
  `sanitize_cell` は現在 report.rs モジュール内プライベート（src/cli/report.rs:270）
  なので、doctor から使うには可視性の調整が必要
- 補間箇所の正確な位置（検証済み）: `display(p)` は src/cli/views/doctor.rs:90-96
  付近（`p.project` または `tilde_path(&p.root)`、tilde_path 本体は doctor.rs:466
  付近）、`worst.file` は doctor.rs:124-131 付近（`ThrashLine.file` の字段、
  src/core/optimize.rs:128-132。値の由来は `ThrashEpisode.file()`、
  src/core/thrash.rs:49-51）、`p.project` の scope 指定は doctor.rs:132-139 付近
- `extract_work_events` の Read 腕（src/adapter/transcript.rs:615-627 付近）も
  同じく生の path/basename を push する。(1) を関数入口側でまとめて当てる形に
  すれば Read 経由の流出も一緒に締まる
- テーブル経由の表示は既に安全: `escape_markdown_cell`（src/cli/report.rs:233-238
  付近）と `format_row`（src/cli/report.rs:251-258 付近）が `sanitize_cell` を
  通す。本 PBI で出力が変わるのは doctor の prose のみを狙う
- エビデンス: pbi/2026-11-21-00-backlog-holistic.md（06 行、RICE と依存）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] 正常系 doctor 出力のバイト同一性を golden / capture diff で確認
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・
  干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
