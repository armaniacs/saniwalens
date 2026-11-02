# PBI: prose の .rs 参照整合 gate を張る（spec 参照の dangling を機械で防ぐ）

## ユーザーストーリー
保守担当者として、spec と doc comment のファイルパス参照が壊れたら機械で教えて
ほしい、なぜなら prose のパス参照は Rust シンボル体系の外にあり、複数の review
を生き延びた dangling 参照（docs/specs/harness.md の 2 箇所）が再生産される
からだ。

## ビジネス価値
- rename / 移動のたびに取り残される dangling 参照が、review ではなく gate で
  検出される（失敗様式が構造的に diff の外で発生するクラスに初めて観測点が
  できる）
- 測定方法: 存在しない src/foo.rs へのバッククォート参照を docs に足すと
  gate が非ゼロで失敗すること。現状（harness.md 修正後）は gate が通ること
  （2026-10-31 のなぜなぜ分析 round2 で 3 立場 x 30 段階、機械スキャン実測
  済みの判断）

## 背景（2026-10-31 のなぜなぜ分析 round2、3 立場 x 30 段階、機械スキャン実測）
- PBI 32（コミット 4ecca9c）で `src/cli.rs` を `src/cli/mod.rs` 等に分割した。
  最終 review の修正コミット 96f7339 で stale 参照 6 箇所を直したが、
  **docs/specs/harness.md:202 と :226** が残存した（観察済みだが範囲外）
  - :202「`cli.rs` guards the store against a cross-harness write and then
    calls the seam」、:226「so `cli.rs` gains no dispatch arm」。`src/cli.rs`
    は不存在（ls 実測）。意味は現行でも真（cli/mod.rs の `run_analyze`
    （:2156 付近）が `guard_harness`（session.rs:229 付近）を呼んでから seam
    を呼ぶ）― 死んでいるのは名指し（錨）だけ
  - タイムライン: :202 は b6fd796（10-30）で誕生（当時は真）→ 4ecca9c で
    stale 化（harness.md 無変更）→ 96f7339 が列挙 6 件のみ修正 → 現在
- 根本原因: prose のファイルパス参照が Rust シンボル体系の外にあり、結合を
  検証する機構が何も無い。全保障（review・規約・checklist）が「注意ベース +
  diff スコープ」で、失敗様式が構造的に diff の外（無変更ファイル内の
  dangling 参照）で発生するためどの保障も原理的に届かない
- **spec-sync.md 自身も stale**: `.claude/rules/spec-sync.md` は 2026-06-14
  （b2eace6）以来 1 度も変更されておらず、Code→Spec map に harness.md 行が
  存在しない。map に従う agent は harness 変更を architecture.md と cli.md に
  配線し harness.md に着地しない（96f7339 が cli.md / architecture.md に触れ
  harness.md に触れなかった事実と一致）
- 機械スキャン実測: `grep -rn "src/cli\.rs" docs/specs/` は 0 件（自然な
  追跡クエリが空振りする pattern スコープの罠）。bare `cli\.rs` で 2 件。
  docs/specs/ 内の .rs 参照棚卸し（監査コマンド実測、19 件）: dead 2
  （cli.rs x 2）、曖昧 3（config.rs x 2、transcript.rs x 1、src/adapter/ 配下
  でのみ解決）、解決 14
- CI は 5 job 全部 cargo ベース、justfile に参照整合なし。docs-charset は
  skill（機械ゲートでなく文字種のみ）
- 隣接指摘: 未着手 PBI 04 / 05 / 06 / 12-15 の「関連コード: src/cli.rs（行番号
  付き）」は履歴ではなく実装指示として腐っている（着手時に実装者を誤導）

## BDD受け入れシナリオ

```gherkin
Scenario: gate が dangling 参照を検出する
  Given docs/specs/ の .md に存在しない src/foo.rs へのバッククォート参照がある
  When gate を実行する
  Then gate は非ゼロで失敗し、miss 一覧を出力する

Scenario: 現状の docs は gate を通る
  Given harness.md の stale 参照が修正済みである
  When gate を実行する
  Then gate は成功する

Scenario: 履歴と fixture は対象外である
  Given docs/pr、docs/exploration、pbi、CHANGELOG、src 内 fixture 文字列に
  古いパス表記がある
  When gate を実行する
  Then gate はそれらを検査せず成功する（履歴は当時の記録として不変）
```

## 受け入れ基準
- [x] `docs/specs/harness.md:202` を現行錨に張り替える（単純置換で終わらせず、
  guard の所在も述べる形: `cli/mod.rs` の `run_analyze` と `session.rs` の
  `guard_harness`）。:226 を `cli/mod.rs` に張り替える
- [x] 参照整合 gate を実装する: docs/specs/*.md + README.md + AGENTS.md 内の
  バッククォートで括られた .rs 参照を抽出し、src/ と src/adapter/ で存在
  確認、miss で非ゼロ終了 + miss 一覧の出力
- [x] gate は **cargo test** として実装する（`tests/` 配下の 1 ファイル。
  CI の 5 job が全 cargo ベースで `just` に依存しない。CARGO_MANIFEST_DIR か
  workspace 相対で docs を解決する）
- [x] 曖昧 3 件（config.rs x 2、transcript.rs x 1）の解決規則を固定し、
  docs/specs/ 内の表記を統一する（src/ 配下か src/adapter/ 配下かを docs 側
  で明記）
- [x] `.claude/rules/spec-sync.md` 更新: MUST-change に「改名 / 移動した
  ファイルへの prose 参照（src doc comments、docs/specs/、README、AGENTS）」
  を明記し、Code→Spec map に harness.md 行を追加する
- [x] gate のスコープを厳守する: docs/pr、docs/exploration、pbi、CHANGELOG、
  src 内 fixture 文字列（adapter/transcript.rs、adapter/opencode.rs の
  `cli.rs` はテストフィクスチャ）は対象外
- [x] 未着手 PBI 04 / 05 / 06 / 12-15 の「src/cli.rs」指示を同一 docs pass で
  揃える価値を判断する（触れるなら実装指示として現行パスに。完了 PBI と
  CHANGELOG は履歴として触れない）
- [x] `just check` と `cargo test` が green（gate 通過）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- gate 自身が検証対象（dangling 参照で失敗、現状 docs で通過）

### 統合テスト
- cargo test フルスイート green（gate 含む）

### 単体テスト
- 参照抽出の境界（バッククォート内のみ、コードブロック内の扱いを pin）

## 実装アプローチ
- **Outside-In**: gate の失敗パス（dangling 参照の fixture）から Red を
  確認し、harness.md 修正で Green、規約更新と docs 統一で仕上げる
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0: R=2 / I=2 / C=3 / E=1日。2 行の doc fix のみなら
PBI 過剰だが、gate と規約更新を含むため PBI 1 本が妥当 ― PBI 31 が 1 行
インデックスでも PBI 化の前例）

## 技術的考慮事項
- 依存関係: PBI 33 / 34 と独立だが、gate は docs/specs/*.md を走査するため
  **バッチ内で最後に着地する**（PBI 33 の cli.md 更新と PBI 34 の
  architecture.md 更新の後）
- テスタビリティ: gate 自身がテスト。dangling 参照の fixture はテスト内文字列
  （実 docs を書き換えない）
- 非機能要件: プライバシー（fixture 無し）、履歴ファイルは不変

## 実装者向け注記

### 現状コードの確認（2026-10-31 なぜなぜ分析で機械スキャン実測済み）
- `docs/specs/harness.md:202` と `:226` の現行文（stale 錨の実文）。修正は
  単純置換でなく guard 所在も述べる形に
- `.claude/rules/spec-sync.md`: Code→Spec map の現行行（harness.md 行が無い）
- 曖昧 3 件の所在: config.rs x 2、transcript.rs x 1（docs/specs/ 内）。
  gate 実装時に解決規則を固定する
- CI の 5 job と justfile の現行レシピ（gate の配線先を確認）

### 実装手順
1. gate テスト（tests/ 配下 1 ファイル）を Red で書く（docs/specs/harness.md
   の現行 stale 参照が miss として検出されることを確認）
2. harness.md:202 / :226 を現行錨に張り替えて Green にする
3. 曖昧 3 件の解決規則を固定し docs/specs/ 内を統一する
4. `.claude/rules/spec-sync.md` に MUST-change と harness.md 行を追加する
5. 未着手 PBI の stale 指示を判断して対応する
6. `cargo test` → `just check`（直接実行）→ gate 通過を確認

### 落とし穴
- gate の抽出はバッククォート内の .rs のみ（コードブロック・インライン
  コードの混在を pin する境界テストを書く）
- src 内の `cli.rs` はテストフィクスチャ文字列（adapter/transcript.rs、
  adapter/opencode.rs）で doc 参照ではないため対象外。gate が誤検出しない
- 履歴（docs/pr、docs/exploration、pbi、CHANGELOG）は対象外に allowlist する。
  「当時の記録」を書き換えない
- `.claude/rules/` の .md も docs-charset の対象（CP932 範囲、em dash 禁止）

## Definition of Done
- [x] 全 BDD シナリオが gate テストとして実装されパスする
- [x] harness.md の stale 錨が解消され gate が green で着地
- [x] `just check` と `cargo test` が green
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/harness.md、.claude/rules/spec-sync.md、docs/specs/ 内の
  曖昧参照更新済み
