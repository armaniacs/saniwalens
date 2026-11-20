# バックログ: 大局的コード改善ラウンド (2026-11-09)

holistic-code-review 4班並列（DRY / SoC / 拡張性 / 堅牢性）の実コード裏取り済み指摘のみをPBI化した。adversarial-review 17件（2026-11-08台帳）は対象外（重複計上しない）。

## 出所

- DRY班: `analyze()` 三重複写 + site表 + `RunLineRenderer` 漏れ、view描画儀式14箇所、窓語彙分裂
- SoC班: `src/cli/mod.rs` god化、`core/findings.rs` との二重組立、契約の曖昧型
- 拡張性班: 4thハーネス約20タッチ点、`RunLineRenderer` のopencode型漏れ、table/JSON二重描画
- 堅牢性班: briefing無制限組立、fingerprint TOCTOU、秘密tempのRAII欠落

## 順位表（RICE同一基準・依存反映済み）

| 順位 | 候補 | RICE | 内訳 (R/I/C/E) | 依存・逸脱理由 |
|------|------|------|----------------|----------------|
| C9 | briefing上限（SQL caps+バイト上限） | 8.0 | 8/3/100%/3 | なし。最優先（OOM+秘匿+コストの実害） |
| C11 | fingerprint読後再stat | 4.0 | 5/3/80%/3 | なし。増分実行のたび影響 |
| C1 | harness `analyze_template` 共通化 | 3.2 | 8/2/100%/5 | C2/C3の先行（同一ファイル群） |
| C10 | 秘密temp RAII+既定timeout | 3.2 | 4/2/80%/2 | C9の後（mod.rs/headless.rs同ファイル） |
| C2 | `RunLineRenderer` 汎化+guide重複削除 | 3.0 | 3/2/100%/2 | C1の後 |
| C3 | XDG共通化+opencode影loader削除 | 3.0 | 3/1/100%/1 | C1の後 |
| C4 | `render_view` 共通化+5ビュー移行 | 2.0 | 10/1/100%/5 | C6の先行 |
| C7 | findings単一組立化 | 1.92 | 6/2/80%/5 | C8の先行 |
| C5 | `WindowArgs` 単一化 | 1.33 | 5/1/80%/3 | なし |
| C6 | mod.rs分割（deliver+描画→views） | 0.7 | 7/1/80%/8 | C4の後 |
| C8 | 契約rogen（newtype+`(&Store,&Harness)`） | 0.25 | 4/1/50%/8 | C6/C7の後、最後 |

純RICE降順からの逸脱は依存のみ。上記表の通り。

## 依存マップ

- C1 → C2, C3（harness.rs / opencode.rs同一ファイル群）
- C4 → C6（描画ヘルパー先行）
- C9 → C10（mod.rs / headless.rs同一ファイル群）
- C7 → C8（組立先行、型後）

## 台帳送り（今回扱わない）

- なし。全11件をPBI化する。

## 継続ラウンド引継ぎ

- `pbi/`直下の残はdeferral済み（06/19）のみ。Oct-31 CIバッチ（01/07/08/11/26/28）は実装・検証のうえ全件アーカイブ済み（08のdependabot初回更新PRの外部確認はPR発生時に実施）。外部確認・実データ入手が再検討トリガー。

## アーカイブ履歴（本台帳以降に着地した分）

| PBI | RICE | 結果 |
|---|---|---|
| [fix-chrono-to-jiff-migration](archived/2026-11-17-01-fix-chrono-to-jiff-migration.md) | — | chrono（RUSTSEC-2026-0543）を jiff に置換し deny ゲートを回復。出力はバイト同一（PR #41、CHANGELOG 0.6.5、2026-11-19 マージ）。cargo test 772 green |
