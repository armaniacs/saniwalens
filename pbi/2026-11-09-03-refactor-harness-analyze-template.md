# PBI C1: harness analyze_template 共通化

- Priority rank: 3
- RICE: 3.2 (Reach 8 / Impact 2 / Confidence 100% / Effort 5)
- Estimate: 5
- 種別: リファクタ (振る舞い保存、出力バイト同一)

## 背景

`analyze` のテンプレート組み立てが harness 別に重複している。

- global サイトテーブル: `src/adapter/opencode.rs:465-482` vs `src/adapter/kilocode.rs:530-556` (先頭 5 行同一)
- project サイト: `:508-522` vs `:573-588`
- 同型の組み立て: `src/adapter/claude.rs:99-131`
- ingest 末尾が 3 件で逐語重複: `claude.rs:214-274`、`opencode.rs:891-946`、`kilocode.rs:791-877` (IngestBatch + record_ingested_file + set_meta + AnalyzeStats)
- fingerprint ガードが 3 件で重複: `claude.rs:182-189`、`opencode.rs:753-755,832`、`kilocode.rs:734-768`

新規 meta キー追加時に 3 箇所編集が必要で、抜け漏れの温床になっている。

## ゴール

harness 別の `analyze` 出力がバイト同一のまま、テンプレート組み立て・ingest 末尾・fingerprint ガードを共通化し、meta 追加は 1 箇所の編集で済むようにする。

## BDD シナリオ

### 1. harness 別 parity/golden が全件通る
- Given 各 harness (claude / opencode / kilocode) の既存 fixture
- When 共通化後の `analyze` を実行する
- Then 共通化前の golden 出力とバイト同一である

### 2. 新規 meta キーは 1 箇所の編集で全 harness に反映される
- Given 共通テンプレートに meta キーを 1 件追加する
- When 各 harness の `analyze` を実行する
- Then 3 harness の出力に同一キーが含まれ、harness 別分岐の追加編集が不要である

## 受け入れ条件

1. 対象重複 (サイトテーブル / ingest 末尾 / fingerprint ガード) が共通関数・テンプレートに集約されていること
2. per-harness の parity / golden テストが全 harness でグリーンであること
3. 出力が共通化前とバイト同一であること (golden 差分ゼロ)
4. 新規 meta キー追加の編集箇所が 1 箇所であること (レビューで確認)
5. 既存 e2e が全件グリーンであること
6. harness 固有知識が adapter 境界外に漏れていないこと

## テスト戦略 (t-wada)

- Red: harness 別 fixture に対する parity テスト (共通化前の出力を golden 化) を追加し、共通化の前後差分を検出できる状態にする
- Green: 共通化を実施し、parity + 既存 e2e をグリーンにする
- Refactor: 共通テンプレートの命名・配置を整え、重複呼び出し側を薄くする

## DoD

- [ ] 上記受け入れ条件 1-6 を満たす
- [ ] parity テスト + 既存 e2e がグリーン
- [ ] 重複 3 系統の削除差分をレビュー済み

## 検証記録 (B2統合・2026-11-09)

- 実装: `ParsedSession`＋`analyze_template` (`src/adapter/harness.rs`)、3ハーネスはSite行＋parseのみに縮小。meta blockは単一化済み、opencode影loaderはC3に残す。
- 境界検証: fmt OK、clippy 0 errors、lib 640 passed、`analyze_template_parity` 3 passed（stats/catalog/metaピン、re-blessなし）、goldens 3/3、`fingerprint_restat` 6/6（C11ゲート不変）。
- ついでの統合修正: `analyze_template` のtoo_many_argumentsに `#[allow]`＋理由注記（単一呼出しの集約点のため構造体化しない）。
