# バックログ: 大局的コード改善ラウンド2 (2026-11-21)

holistic-code-improvement スキルによる第2ラウンド。4班並列レビュー（DRY / SoC / 拡張性 / 堅牢性）の実コード裏取り済み指摘のみをPBI化した。第1ラウンド（2026-11-09 台帳、C1-C11）と adversarial ラウンド（2026-11-08 台帳、17件）で閉じたテーマは差分スコープにより再レビュー対象外。対象差分: 2026-11-10 以降の 68 コミット（src/tests で 106 ファイル、+15,173/−3,549）と残存する全体構造課題。

進行中の 06（4th harness、discovery 待ち）と 19（MCP schema 計測、外部条件待ち）は着手禁止ゲート継続のため本ラウンド対象外（2026-11-09 台帳の台帳送りを維持）。

## 順位表（RICE同一基準・依存反映済み実行順）

| 順位 | PBI | RICE | 内訳 (R/I/C/E) | 依存・根拠 |
|------|-----|------|----------------|-----------|
| 01 | [store-schema-reconcile-guard](2026-11-21-01-fix-store-schema-reconcile-guard.md) | 24.0 | 4/3/100%/0.5 | なし。実害（未分類表の列追加で全ユーザー INSERT 失敗）封じ込め |
| 02 | [usage-token-clamp](2026-11-21-02-fix-usage-token-clamp.md) | 18.0 | 6/3/100%/1 | なし。store.rs には触れない（05 と排他） |
| 03 | [surface-kind-predicates](2026-11-21-03-refactor-surface-kind-predicates.md) | 8.0 | 4/1/100%/0.5 | なし |
| 04 | [advisor-language-vocabulary](2026-11-21-04-refactor-advisor-language-vocabulary.md) | 4.0 | 4/0.5/100%/0.5 | なし。session.rs は 13 より先 |
| 05 | [core-row-ownership](2026-11-21-05-refactor-core-row-ownership.md) | 10.0 | 5/1/100%/0.5 | 01 の後（store.rs 排他） |
| 06 | [doctor-prose-sanitize](2026-11-21-06-fix-doctor-prose-sanitize.md) | 16.0 | 4/2/100%/0.5 | 02 の後（transcript.rs 排他） |
| 07 | [util-layer-placement](2026-11-21-07-refactor-util-layer-placement.md) | 5.0 | 5/1/100%/1 | 04 の後（session.rs 排他）。09 と harness.rs 排他（07 先） |
| 08 | [store-range-params](2026-11-21-08-refactor-store-range-params.md) | 6.0 | 6/0.5/100%/0.5 | 05 の後（store.rs 排他） |
| 09 | [harness-facts-assembly](2026-11-21-09-refactor-harness-facts-assembly.md) | 2.4 | 6/1/80%/2 | 07 の後（harness.rs 排他） |
| 10 | [guide-toml-upsert](2026-11-21-10-refactor-guide-toml-upsert.md) | 3.0 | 3/1/100%/1 | なし（views/guide.rs 内で閉じる） |
| 11 | [apply-plan-byte-cap](2026-11-21-11-fix-apply-plan-byte-cap.md) | 6.0 | 3/1/100%/0.5 | なし。handoff.rs は 12 と排他（11 先） |
| 12 | [optimize-apply-prologue](2026-11-21-12-refactor-optimize-apply-prologue.md) | 2.0 | 5/1/80%/2 | 07（session.rs）・11（handoff.rs）の後 |
| 13 | [session-responsibility-split](2026-11-21-13-refactor-session-responsibility-split.md) | 1.3 | 5/1/80%/3 | 12 の後（session.rs 排他） |
| 14 | [temp-name-stem](2026-11-21-14-refactor-temp-name-stem.md) | 3.0 | 3/0.5/100%/0.5 | 12 の後（handoff.rs 排他） |

純 RICE 降順からの逸脱はファイル排他（直列化）のみ。同一 RICE 内はリスク軽減効果を優先（C10 > C14、C7 > C13）。バッチ構成（同一ディレクトリ + ファイル排他）:

- バッチ1（並列）: 01 / 02 / 03 / 04
- バッチ2（並列）: 05 / 06 / 07
- バッチ3（並列）: 08 / 09 / 10
- バッチ4（単独）: 11
- バッチ5（単独）: 12
- バッチ6（並列）: 13 / 14

## 依存マップ

- 01 → 05 → 08（store.rs）
- 02 → 06（transcript.rs）
- 04 → 07 → 12 → 13（session.rs）
- 07 → 09（adapter/harness.rs）
- 11 → 12 → 14（cli/handoff.rs）

## 台帳送り（今回扱わない）

- 進行中 2 件のゲート継続: 06（4th harness、discovery で実データ有無の確認が必要）、19（MCP schema 計測、安全な静的 schema ソースの出現待ち）
- 積み残しテーマ（再検討トリガー: 次ラウンド予約）:
  - guide TUI の UI ローカライズ方針（views/guide.rs のプロンプト文のみ日本語ハードコード。UI ローカライズ決定時の初手）
  - advisor prompt 内の schema crib（core/optimize.rs 内、store schema の手写し）の pin テスト無し。schema 進化時に陳腐化する
  - transcript JSONL・opencode DB の全量一括読みのメモリ上限（巨大ファイル時のみ顕在化、低）
  - cleanup `dir_size` の無境界再帰（ユーザー自身の state 配下のみで低）
  - DRY 班の小物: waste.rs suggestion 式の二重計算、CatalogEntry→CatalogRow 8 フィールドマッピング二重化、opencode/kilocode の同型 truncate、guide_entry 3 フラグ解析のテーブル駆動化
  - 堅牢性班の小物: waste.rs 冗長ペア再照会の project 非比較（正しさの問題）、headless `Instant::now() + limit` パニック（vulnhunt 済・格下げ済みの再録扱い）

## 出所

- holistic-code-improvement ラウンド2（ブランチ 1121a）。holistic-code-review スキルは未導入のため、4班並列サブエージェント（DRY / SoC / 拡張性 / 堅牢性、全指摘に file:line 実証）で代替実施。報告書間の矛盾（cache 計測コードの存在）は機械確認で棄却済み

## アーカイブ履歴（2026-11-21: 全14件 実装・検証・アーカイブ済み）

| PBI | RICE | 結果 |
|---|---|---|
| [01-store-schema-reconcile-guard](archived/2026-11-21-01-fix-store-schema-reconcile-guard.md) | 24.0 | sqlite_master 列挙で未分類表を移行時に拒むガード（`ensure_all_classified`）、subagent_runs を MIGRATED に追加、列追加ドリフトの行存続テスト新設。spec 同期（storage.md）込み。8f09707 |
| [02-usage-token-clamp](archived/2026-11-21-02-fix-usage-token-clamp.md) | 18.0 | 摂取境界 3 アダプタで 10^9/フィールドのクランプ（超過は 0 扱い + 1 回の stderr 警告）、core 加算を saturating_add に統一。アナライザバージョン bump に伴い parity pin を定数導出化。spec 同期（session-format 3 件）込み。ac678cf |
| [03-surface-kind-predicates](archived/2026-11-21-03-refactor-surface-kind-predicates.md) | 8.0 | KIND_TRAITS 表 + 3 述語（measurable/describable/never-fires）を core に集約、waste.rs の文字列リストを除去。出力バイト同一（waste golden 5 件）。8acdce3 |
| [04-advisor-language-vocabulary](archived/2026-11-21-04-refactor-advisor-language-vocabulary.md) | 4.0 | AdvisorLanguage に ALL/label()/label_list()、ロケール投票を対照表化。help 単一ソース pin は行予算を守るため help golden 側に置く。出力バイト同一。5ae8557 |
| [05-core-row-ownership](archived/2026-11-21-05-refactor-core-row-ownership.md) | 10.0 | 共有 row 型 4 個を store から core/findings.rs へ移し、core→store 依存逆転を解消。store は pub use 再エクスポート。テスト名 diff 空。0707da9 |
| [06-doctor-prose-sanitize](archived/2026-11-21-06-fix-doctor-prose-sanitize.md) | 16.0 | extract_work_events に whitespace-collapse + 200 字 truncate を適用し、doctor 補間値 4 箇所を sanitize_cell 経由に。E2E で ESC/BEL が stdout に到達しないことを pin。444b4fb |
| [07-util-layer-placement](archived/2026-11-21-07-refactor-util-layer-placement.md) | 5.0 | home_dir/xdg_from_env を session env 家へ、state_base を切り出して handoff の手書き複製を統合、store の fs ヘルパー export を廃止（deliver.rs が所有）。xdg parity 6 件 green。c42ac82 |
| [08-store-range-params](archived/2026-11-21-08-refactor-store-range-params.md) | 6.0 | 9 箇所の bind+LIMIT 組立を range_params 1 ヘルパーに統合。SQL 文字列 diff 空。295c1f7 |
| [09-harness-facts-assembly](archived/2026-11-21-09-refactor-harness-facts-assembly.md) | 2.4 | 静的コピー 11 メソッドを HarnessFacts + facts() 1 アームに集約、PBI 07 ブリッジを削除、harness.md に実タッチ全数を記録。E2E golden 3 ハーネス バイト同一。6f86ebe |
| [10-guide-toml-upsert](archived/2026-11-21-10-refactor-guide-toml-upsert.md) | 3.0 | TOML 行編集 2 骨格を upsert_toml_line（InsertStrategy enum）に統合、HEADER 二重定義を解消。既存 54 golden 無変更 green。a06239c |
| [11-apply-plan-byte-cap](archived/2026-11-21-11-fix-apply-plan-byte-cap.md) | 6.0 | PLAN_MAX_BYTES（64KB）で plan 抽出とログ読みの両側を打ち切り、超過を stderr 明示。read_capped は UTF-8 文字境界セーフ。既存 handoff 110 テスト無変更 green。adcabff |
| [12-optimize-apply-prologue](archived/2026-11-21-12-refactor-optimize-apply-prologue.md) | 2.0 | プロローグを prepare_optimize 1 関数に統一、「store: refreshed just now」文面を refreshed_just_now_line に集約。mod.rs は予算内（1134/1135）。出力バイト同一。d7d1428 |
| [13-session-responsibility-split](archived/2026-11-21-13-refactor-session-responsibility-split.md) | 1.3 | プレゼンテーション（Style/freshness/language_decision）を report.rs へ、env/locale 解決を session_env.rs へ分離。session.rs 1756→817 行。出力バイト同一。783c05f |
| [14-temp-name-stem](archived/2026-11-21-14-refactor-temp-name-stem.md) | 3.0 | 一時名の衝突耐性イディオム（pid+nanos+SEQ）を unique_temp_stem() 1 箇所に集約、2 委譲先は整形のみ。3 形式ともバイト同一。56be095 |

最終ゲート: cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings / cargo test（41 suite、1,060 passed）/ cargo build --release、全 green。バッチ境界（6 バッチ）ごとに fmt+clippy+test を実行し、バッチ境界でしか出ない失敗を 2 件検出・修正（analyzer_version pin の未追従、cli/mod.rs 行予算超過 — 両方とも統合側が修正）。

## アーカイブ履歴（adversarial レビュー追加分・autonomous-task-closer ラウンド）

adversarial-code-review（fork/main..1121a 差分、4 チャンク × 2 視点、反証 25 件で SURVIVES 22）の指摘を pbi-create-bdd-workspace 流儀で 13 PBI に構造化し（2026-11-21-15〜27）、closer 流儀で実装・アーカイブする。台帳はこのセクションに追記型で集約する。

| PBI | 重大度 | 結果 |
|---|---|---|
| [15-terminal-output-sanitization](archived/2026-11-21-15-fix-terminal-output-sanitization.md) | 高 | sanitize_cell を Cc 全域（\n\t 除く）+ Cf bidi/ゼロ幅除去に強化、doctor_agent_lines の agent 名を sanitize 経由に、E2E fixture に C1/bidi/CR 投入。37a4ba5 |
| [17-timestamp-gap-arithmetic](archived/2026-11-21-17-fix-timestamp-gap-arithmetic.md) | 中 | span_end と duration_sec を saturating_sub 化（opencode の part ts 素通しの隣接極端ペアで debug panic / release wrap）。span.rs コメントに ts クランプ不採用の裁定を記録。4ccb919 |
| [19-frozen-read-only-seam](archived/2026-11-21-19-fix-frozen-read-only-seam.md) | 中 | optimize/guide inline の frozen 糸口を ReadOnly seam に切替（PBI 08-12 の閉じ残し）+ frozen 時の fresh 度行。テストを seed 方式へ移行。8641d3b |
| [20-timeout-direct-deadline](archived/2026-11-21-20-fix-timeout-direct-deadline.md) | 中 | Headless+verbose に deadline 機構を適用、direct は明示 timeout で警告。spec 同期（cli.md timeout 節 + frozen 契約の全体化）込み。c329ab0 |
| [16-briefing-tool-name-injection](archived/2026-11-21-16-fix-briefing-tool-name-injection.md) | 中 | by tool 行を excerpt と同一の単行化 + 300 字 cap 契約に（store 不変、version bump 不要）。213f58c |
| [18-corrupt-transcript-resilience](archived/2026-11-21-18-fix-corrupt-transcript-resilience.md) | 中 | 不正 UTF-8 行のスキップ計数、usage 字段の寛容パース（行保持）、drop 計数 3 ハーネス統一。CLAUDE version 11 bump（消滅行の回復）。spec 同期（session-format 3 件）込み。8a4c43c |
| [22-guide-toml-header-recognition](archived/2026-11-21-22-fix-guide-toml-header-recognition.md) | 中 | table_header の行内コメント/末尾空白対応、同名テーブル二重定義の追記拒否 + 警告。既存 62 pin バイト同一。dde39c0 |
| [26-migrate-guard-transaction](archived/2026-11-21-26-fix-migrate-guard-transaction.md) | 低 | version guard を BEGIN IMMEDIATE 内に移動、busy_timeout 5s。防御的強化（現行 SCHEMA_VERSION=1 で発生不能）。spec 同期（storage.md）込み。5e94aef |
| [21-waste-glob-computation-bound](archived/2026-11-21-21-fix-waste-glob-computation-bound.md) | 中 | ** 展開をメモ化 DP + MatchBudget 化（超過 rule は除外して名指し警告）、target 照会に LIMIT 5000。旧マッチャで 258 秒 → 0.35 秒の e2e 実測。spec 同期（cli.md waste 行）込み。00d20a7 |
| [23-kilocode-window-scan-quad](archived/2026-11-21-23-fix-kilocode-window-scan-quad.md) | 中 | 請求窓帰属を partition_point 2 回に置換（旧実装で最悪 283 秒 → O(n log n)）。帰属結果バイト同一（golden + 境界 pin 5 件）。20c5718 |
| [24-plan-truncation-visibility](archived/2026-11-21-24-fix-plan-truncation-visibility.md) | 中 | 観測フェーズで指摘を実測確認（無印 plan + 末尾欠け）のうえ、capture から plan 組立まで打ち切り flag を運び plan 末尾に marker 追記。spec 同期（cli.md --apply 節）込み。14ad2f0 |
| [25-private-dir-owner-verification](archived/2026-11-21-25-fix-private-dir-owner-verification.md) | 低 | 既存 dir の group/other ビット検出と chmod 拒否を stderr 警告に（warn, never silently inherit）。正常系バイト同一。spec 同期（storage.md permission 節）込み。bb246c9 |
