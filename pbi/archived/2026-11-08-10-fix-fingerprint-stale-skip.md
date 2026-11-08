# PBI: fingerprint の秒切り捨てによる stale-skip を直す

## ユーザーストーリー
増分 analyze を使う saniwalens ユーザーとして、同一秒内の同一サイズ編集が
取り込まれないままスキップされることがなく、最新の transcript が常に
反映されてほしい。なぜなら取り込み漏れは分析データ全体を古くし、
以降の wedge 指摘の信頼を損なうからだ。

## 優先度
- 順位: 10 / 全17件
- RICEスコア: 4.0（Reach=5 / Impact=1 / Confidence=80% / Effort=1日）
- 根拠: 毎回の増分実行で stale な分析データが残る。影響は全ハーネスに及ぶ。

## BDD受け入れシナリオ
Scenario: 同一秒・同一サイズの編集は再取り込みされる
  Given 前回 analyze で取り込み済みの transcript があり
  And   その transcript が同一秒内に同一サイズで編集された
  When  ユーザーが増分 analyze を実行する
  Then  その transcript はスキップされず再取り込みされる
  And   ストアの当該セッション行が新しい内容に更新される

Scenario: 未変更の transcript は従来通りスキップされる
  Given 前回 analyze から変更の無い transcript がある
  When  ユーザーが増分 analyze を実行する
  Then  その transcript はスキップされる
  And   スキップ数がサマリ行に計上される

Scenario: 同一秒編集でも analyzer バージョンが回れば再取り込みされる
  Given fingerprint が一致している transcript がある
  But   analyzer バージョンが上がっている（抽出意味論の変更）
  When  ユーザーが analyze を実行する
  Then  その transcript は再取り込みされる

## 受け入れ基準
- [x] 同一秒内の同一サイズ編集が増分実行でスキップされないこと
- [x] 未変更ファイルのスキップ（高速な増分実行）が維持されること
- [x] analyzer バージョン回転時の全体再取り込みが維持されること
- [x] fingerprint の意味論変更が docs/specs/storage.md に反映されること
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green
  - 注: fmt --check OK、clippy OK、harness/store スコープと新規テストは green。
    フル `cargo test` は委任制約（スコープ限定実行）のため未実行 — レビュー時に実行のこと。

## テスト戦略
- 単体: fingerprint が同一秒内の mtime 差を区別すること（nanos 粒度または
  content-hash フォールバックのいずれかを固定）。stat 不能時は従来通り
  無条件取り込み・未記録であること
- 統合: 同一秒・同一サイズで編集された合成 transcript が再取り込みされ、
  未変更の合成 transcript がスキップされること（claude 側と kilocode 側の
  両スキップ経路）
- E2E: 2 回連続の増分 analyze で、2 回目に同一秒編集を加えた場合のみ
  skipped にならず sessions として再計上されること

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 原因は粒度の不一致: src/adapter/harness.rs:548-556 の file_fingerprint は
  mtime を whole seconds に切り捨て（`.as_secs() as i64`）、一方
  src/store.rs:1424-1431 の is_ingested は (path, mtime, size) の完全一致で
  スキップ判定する。スキップ呼び出しは src/adapter/claude.rs:179-186 と
  kilocode 側 :715-724（task_fingerprint 経由で file_fingerprint に合流）。
  同一秒・同一サイズの編集は fingerprint が不変のため、fingerprint 変更か
  analyzer バージョン回転までスキップされ続ける
- 修正方向: timestamp の粒度を上げる（nanos）か、content-hash フォールバックを
  足す。どちらを選んでも storage.md の fingerprint 契約（`docs/specs/storage.md`
  の ingested_files 規約）と同時に更新する（spec-sync 規約: 同一変更内）
- kilocode の task_fingerprint（2 ファイルの max-mtime + size 合算）も
  file_fingerprint の粒度に依存するため、修正は harness.rs の一点で両経路に
  効くことを確認する。opencode 経路（time_updated_ms + message_count）は
  対象外であることをテストで固定する
- 回帰ピン: 同一秒編集の再取り込みを固定するテストを追加する（現状その
  ケースのテストは無い）。並行書き込み中の read（pre-read stat の規約、
  store.rs:1433-1435 のコメント）は崩さない
- エビデンス: docs/specs/storage.md（ingested_files と analyzer version 規約）
- 関連コード: src/adapter/harness.rs（file_fingerprint）、src/store.rs
  （is_ingested / record_ingested_file）、src/adapter/claude.rs
  （incremental skip）、src/adapter/kilocode.rs（task_fingerprint と skip）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み
