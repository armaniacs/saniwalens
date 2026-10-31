# PBI: 参照整合 gate のスキャンリストに .claude/rules を足す（同クラスの錨が規約ファイル内で不可視）

## ユーザーストーリー
保守担当者として、規約ファイル内のパス参照も参照整合 gate に機械で検証されて
ほしい、なぜなら gate が機械化したのと同じ錨クラスが .claude/rules 内に 8 件
あり、test ファイル改名時に gate だけは失敗しないからだ。

## 優先度
- 順位: 36 / 全候補数 7
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1日）
- 根拠: 最小 Effort（~15 行 Rust + ~4 行 doc + テスト 2 行）で機械化クラスの
  連続保護を得る。以降の全 docs 系 PBI（41 含む）の土台になるため最初に着地。
  分析の E=0.5 日読みなら RICE 24.0

## ビジネス価値
- rename のたびに取り残される dangling 参照が、規約ファイル内でも gate で
  検出される（同クラスの錨 5 件が解決、latent miss 3 件が re-point される）
- 測定方法: .claude/rules 内の `.rs` 錨を改名すると gate が非ゼロで失敗する
  こと。現行（re-point 後）は gate が通ること

## 背景（round2 最終 review Important 1+2 + なぜなぜ分析 2026-11-01 round3、
3 立場 x 30 段階、機械スキャン実測）
- gate 本体 tests/prose_reference_gate.rs（279 行、#[test] 実測 7 件）。
  scan_files は :106-118（README.md + AGENTS.md を無条件リスト化、docs/specs/
  *.md は read_dir+sort）。.claude/rules/*.md は含まれない
- .claude/rules 内 .rs 錨 棚卸し（grep 実測）: 機械化クラス 5 件が解決
  （cli-view-addition.md:9 `src/cli/mod.rs`, :13 `tests/cli_mod_line_budget.rs`,
  :25 `src/cli/trend.rs` + spec-sync.md:53,70 `tests/prose_reference_gate.rs`）
  + latent miss 3 件（cli-view-addition.md:24,28,29 の bare `mod.rs` ―
  is_rs_path が anchor と判定するが 3 基底（root/src/src-adapter）に mod.rs
  無しの ls 実測 → スキャン拡張で 3 件 miss になる）
- 根本原因: gate のスキャンリストが発見時の観測範囲（docs/specs スコープ、
  PBI 35:38-44）から派生し、「現行 prose」の領域定義（AGENTS.md:73 が rules を
  含める）から派生していない。矛盾する 2 つの current prose 定義（gate doc
  comment :18 vs AGENTS.md:73）を潰す機構とカバレッジ assert が無い
- PBI 35:147 は rules を docs-charset 対象と認識していたのに gate 対象から
  漏れた（認識と実装の断絶）

## BDD受け入れシナリオ

```gherkin
Scenario: 規約ファイル内の dangling 参照を gate が検出する
  Given .claude/rules/ の .md に存在しない tests/foo.rs へのバッククォート参照がある
  When gate を実行する
  Then gate は非ゼロで失敗し、file:line 付き miss 一覧を出力する

Scenario: 現状の docs は gate を通る
  Given bare mod.rs x 3 が cli/mod.rs に re-point 済みである
  When gate を実行する
  Then gate は成功する

Scenario: double-backtick は anchor でない
  Given .md に ``src/foo.rs`` 形式の記述がある
  When gate を実行する
  Then gate はそれを anchor として扱わない（単体テストで pin）
```

## 受け入れ基準
- [ ] scan_files（tests/prose_reference_gate.rs:106-118）に
  `.claude/rules/*.md` を追加する（docs/specs と同じ read_dir 経路の再利用）
- [ ] `the_scan_list_is_current_prose_only`（:250-278）と doc comment 3 箇所
  （:1-2, :16-19, :103-105）と format_misses 文言（:139）を同一変更で更新する
  （更新しないと PBI 35 の 7 gate テストが red）
- [ ] cli-view-addition.md:24,28,29 の bare `mod.rs` x 3 を `cli/mod.rs` に
  先に re-point する（5d30eca の統一前例どおり所在明記形）
- [ ] double-backtick span 対象外の単体テスト 1 行を足す
  （refs_in_line("``src/foo.rs``") が空を返す assert）
- [ ] README/AGENTS を読めない場合の panic は fail-loud のまま維持する
  （judge 済み ― miss 形式化は anchor 無しの偽情報を miss 一覧に混ぜる）
- [ ] period.rs:739 の None-skip は本 PBI のスコープ外とする
  （PBI 37 が所有）
- [ ] `.claude/rules/spec-sync.md`:50-54 の「current prose」文言を同一変更で
  rules 付きに更新する
- [ ] `just check`（直接実行）と `cargo test` が green（7 gate テスト + 拡張）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- gate 自身が検証対象（rules 内 dangling で失敗、現状 docs で通過）

### 統合テスト
- cargo test フルスイート green（gate 含む）

### 単体テスト
- 抽出境界の pin 追加（double-backtick 1 行）

## 実装アプローチ
- **TDD**: scan_files 拡張を先に Red にし（rules 内の既存錨が miss 検出）、
  bare mod.rs ×3 の re-point で Green、pin 追加と doc 更新で仕上げる
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0。PBI 35 と同水準・同型）

## 技術的考慮事項
- 依存関係: PBI 35（完了済み）が前提。PBI 41 が依存（新規約の .rs 錨を gate
  が検証するようになる）。PBI 37 と独立（period.rs には触れない）
- テスタビリティ: gate 自身がテスト
- 非機能要件: プライバシー（fixture はテスト内文字列）、履歴不変

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で機械スキャン実測済み）
- tests/prose_reference_gate.rs: scan_files :106-118、
  the_scan_list_is_current_prose_only :250-278、doc comment :1-2/:16-19/
  :103-105、format_misses :139
- .claude/rules/cli-view-addition.md:9,13,25（解決錨 3 件）+:24,28,29（bare
  mod.rs ×3）。spec-sync.md:53,70（解決錨 2 件）+:50-54（current prose 文言）

### 実装手順
1. Red: scan_files に rules を足した状態で cargo test → bare mod.rs ×3 が
   miss 検出されることを確認
2. cli-view-addition.md の bare mod.rs ×3 を re-point して Green
3. the_scan_list_is_current_prose_only と doc comment 3 箇所、format_misses
   文言を同一変更で更新する
4. double-backtick の単体 pin を足す
5. spec-sync.md の current prose 文言を更新する
6. 検証（再ビルド後のバイナリで E2E 類も確認）

### 落とし穴
- bare mod.rs ×3 を先に re-point しないと拡張時に 3 件の miss で red
- resolves() の 3 基底（root/src/src-adapter、:65-73）と履歴/src fixture 対象外
  （:17-19, :104-105）を退行させない
- format_misses 文言（:139）は文字列リテラルでテスト検査されない（手動確認）
- docs-charset: 両 rules ファイルの dash は既に ―（U+2015）で統一済み、
  U+2014 再導入禁止

## Definition of Done
- [ ] 全 BDD シナリオが gate テストとして実装されパスする
- [ ] rules 内の機械化クラス錨 5+3 件が gate の検証下に入る
- [ ] `just check`（直接実行）と `cargo test` が green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] tests/prose_reference_gate.rs、.claude/rules/ 2 ファイル更新済み
