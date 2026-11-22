# PBI: analyze が projects dir 不在を空として扱えるようにする

## ユーザーストーリー
新規ユーザーとして、初回の `saniwalens analyze` が失敗せずに成功してほしい、
なぜなら空状態から始められなければツールを使い始められないから。

## 優先度
- 順位: 01 / 全候補数 5
- RICEスコア: 36.0（Reach=6 / Impact=3 / Confidence=100% / Effort=0.5日）
- 根拠: 初回起動は全環境で必ず通る（fresh env・新規ユーザー・CI ランナー）が、
  `~/.claude/projects` が存在しない環境では Err になる。v0.6.6 の Release
  workflow（nix / build 3 ジョブ）で実証済みで Confidence 100%。順位 2 の
  PBI 06（4.5、discovery ゲート付き）と依存関係なし。

## 背景（現状・証拠）
- `src/adapter/claude.rs` の `list_main_transcripts` は
  `fs::read_dir(projects).with_context("read projects dir")?` — projects dir
  （`CLAUDE_CONFIG_DIR/projects`、デフォルト `~/.claude/projects`）が不在の
  環境で NotFound を Err にし、analyze 全体が失敗する。
- v0.6.6 の Release workflow（run 44975508069）で nix / build 3 ジョブが
  `Error: read projects dir .../saniwalens-advisor-language-.../.claude/projects`
  で失敗（tests/advisor_language.rs の seed analyze が不在環境に当たった）。
  テスト側は hermetic seed 修正（96650b8）で対処済みだが、プロダクト側の
  不在時挙動は未修正。
- opencode（SQLite DB 読み）と kilocode（タスク JSON）は同型の不在挙動が
  あるか未観測 — 本 PBI は claude 経路のみ対象とし、他ハーネスの同型挙動は
  着手時に観測して記録する。

## BDD受け入れシナリオ

```gherkin
Scenario: 新規環境の初回 analyze が成功する
  Given CLAUDE_CONFIG_DIR 配下に projects ディレクトリが存在しない
  When  saniwalens analyze を実行する
  Then  成功し、空のストアが作られる
  And   sessions / events は 0 件である

Scenario: 読み取りできない projects ディレクトリは従来どおり失敗する
  Given projects ディレクトリが存在するが読み取り権限がない
  When  saniwalens analyze を実行する
  Then  エラーは "read projects dir" の文脈を名指して失敗する
  And   空のストアを作らずに終了する
```

## 受け入れ基準
- [x] projects dir 不在（CLAUDE_CONFIG_DIR が新規環境を指す）で analyze が
  success を返し、空ストアが作られる（E2E、ScratchDir 環境）
- [x] 同状態で 2 回目の analyze も success（冪等）
- [x] 読み取り権限のない projects dir（同一ユーザーで chmod 0022 再現不可の
  ため、存在する dir の NotFound 以外の read_dir エラー経路を pin）は
  従来どおり Err で文脈を名指す
- [x] fixture 付きの従来ケース（sessions 1 件）は出力バイト同一
- [x] opencode / kilocode の既存挙動は不変（e2e golden 3/3）

## テスト戦略
- E2E: 不在 projects dir での analyze（新規環境初回シナリオ）
- 統合: fixture 付き golden（e2e_golden_harness 3/3 がバイト同一の証人）
- 単体: list_main_transcripts の NotFound → 空スライス、NotFound 以外は Err

## 見積もり
0.5日

## 実装者向け注記
### 現状コードの確認
```bash
grep -n "read projects dir" src/adapter/claude.rs
grep -n "fn default_projects_dir" src/adapter/claude.rs
```
- 最小修正は `list_main_transcripts` の `fs::read_dir` で NotFound を
  空スライスとして扱うこと（`projects` 自体は作らない — 不在の証人として
  保持するか、作るかは実装時に決めてよい。作る場合も 0700 ではなく
  claude_home の既存挙動に従う）。
- NotFound 以外（PermissionDenied 等）は現行どおり Err（誤設定の可視化）。

### 落とし穴
- `analyze_template` は projects dir を catalog 走査にも使う（config 側と
  別解決のため catalog は影響を受けないはず — 影響があれば観測して記録）。
- opencode / kilocode の同型不在挙動をこの PBI で直さない（観測して記録
  するだけ）。

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] e2e golden 3/3（バイト同一の証人）が green
- [x] コードレビュー完了
- [x] spec 同期（docs/specs/harness.md または session-format.md の不在時
  挙動に 1 行追記）
- [x] 台帳の候補節を実施済みに更新
