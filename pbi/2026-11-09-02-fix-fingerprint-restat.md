# PBI C11: fingerprint 読後再 stat

- Priority rank: 2
- RICE: 4.0 (Reach 5 / Impact 3 / Confidence 80% / Effort 3)

## 背景

- `src/adapter/harness.rs:560-568`: `file_fingerprint` は読取前 stat のみで fingerprint を作る
- `src/adapter/claude.rs:182-247`: 読取前の mtime/size で skip 判定 → 読取 → `record_ingested_file` で stamp
- `src/adapter/opencode.rs:753-755,832,829-917`: 非スナップショット DB スキャンで同 pattern
- `src/adapter/kilocode.rs:669-677,734-768,761-848`: 同 fold pattern
- PBI-10 の nanos 対応は競合 window を縮めただけで、読取と stamp の非 atomic 性は残る。解析中に追記されると、古い fingerprint で stamp して次回 skip する恐れがある。

## ユーザーストーリー

増分 `analyze` を使うユーザーとして、解析中に追記されたトランスクリプトを取りこぼしたくない、なぜなら次回も skip されると使用量・コスト集計が欠損するから。

## BDD 受け入れシナリオ

```gherkin
Scenario: 解析中の追記は再 ingest される
  Given ingest 対象ファイルが skip 判定済みで読取中のとき
  When 読取完了後に再 stat した mtime/size が読取前と異なるとき
  Then stamp せず当該ファイルを再 ingest する

Scenario: 不一致時は stamp を中断する
  Given ファイルを読取した直後に
  When 読取前後の fingerprint が一致しないとき
  Then `record_ingested_file` を呼ばず次回 analyze で再処理する
```

## 受け入れ基準

- [ ] 読取後に再 stat し、不一致なら stamp しない（claude / opencode / kilocode 共通）
- [ ] 追記分が次回 analyze で取り込まれる
- [ ] 一致時は従来通り stamp し、余計な再 ingest をしない
- [ ] 不一致検出の単体テストが全 harness 分ある
- [ ] 既存の skip / stamp テストが壊れない

## テスト戦略（t_wada 式・合成 fixture）

- 単体（多数）: 読取前後で mtime/size をずらした合成 fixture で不一致→stamp 中断、一致→stamp 継続を検証（harness 別）
- 統合（中）: 追記シミュレーションで再 ingest されること
- E2E（最小）: 増分 `analyze` 2 回実行で追記分が欠損しないこと
- Outside-In: 不一致ケースの失敗テスト → 実装 → グリーン → リファクタ

## 見積もり

3pt

## 実装者向け注記

- 読取前 stat → 読取 → 読取後再 stat → 一致時のみ stamp の順に直す。`file_fingerprint`（`harness.rs:560-568`）の呼出側を変更し、fingerprint 自体の I/O は増やさない。
- 3 harness の fold/skip 箇所（claude/opencode/kilocode 上記行）を同形修正する。
- PBI-10 と競合しないよう、nanos 比較ロジックは維持する。

## Definition of Done

- [ ] 全 BDD シナリオが自動テストとしてパスする
- [ ] `cargo fmt` / `clippy` / `test` が green

## 検証記録 (B1統合・2026-11-09)

- 実装: `restat_matches` gate (`src/adapter/harness.rs`)、3ハーネスのstampゲート (claude/opencode/kilocode)、spec-sync (`docs/specs/storage.md`)。ついでのpre-existing lint修正2件 (opencode.rs test helperのlifetime/未使用変数、kilocode_files.rs未使用import) を同コミットに含む。
- 境界検証: `cargo fmt --check` OK、clippy 0 errors、lib 636 passed、新規 `fingerprint_restat` 6 passed、`adapter_robustness` 3 passed、quiet pathのbyte同一golden 2件通過。
