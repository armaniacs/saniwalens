# PBI: 純粋 join の placement rule を張る（fetch 済み row 加工の帰属を spec で決める）

## ユーザーストーリー
実装者として、「fetch 済み row を加工するだけの純粋関数」の帰属を決める条項が
spec にあってほしい、なぜなら placement rule の不在で、1 件目のパターン
（SQLite を含まない関数が最大のファイルに吸収され、テストも吸収され、trend が
store の名前空間に依存する）が標準化してしまったからだ。

## 優先度
- 順位: 41 / 全候補数 7
- RICEスコア: 6.0（Reach=1 / Impact=1 / Confidence=3 / Effort=0.5日）
- 根拠: docs-only で最小（rule 張りのみ）。tie-break: 39（実 drift）・40
  （最大構造予防）の後に。2 件目発火義務込みで実効 I を引き上げる。
  PBI 36 に依存（gate が rules を走査するようになってから新規約の .rs 錨を
  書くと gate が検証する）

## ビジネス価値
- 2 件目の純粋 join が store.rs に現れたら同じ PR で core へ移動する義務が
  spec に宿り、pattern が成長する前に止まる（trigger 乗車の refactor は
  marginal RICE 12.0）
- 測定方法: .claude/rules/ に placement rule が着地し、spec-sync map に 1 行
  追加されること

## 背景（なぜなぜ分析 2026-11-01 round3、3 立場 x 30 段階、file:line 実証）
- store.rs が「SQL の知識」を所有するモジュールであり同時に最大のファイル
  （3,924 行 / pub fn 41 / 65 テスト）のため、「fetch 済み row を加工するだけの
  純粋関数」の帰属を決める条項が architecture.md:96-101 の所有表にも
  spec-sync.md の map（:26/:28）にも無い（placement rule の不在）
- trend 着地 PBI 30-02 で placement が実装者のその場の判断に落ち
  （archived/2026-10-30-02:119-120「実装者に任せる」）、1 件目のパターン
  （store.rs 吸収、テスト吸収、trend.rs:10 の store 名前空間依存）が標準になった
- tdd.md:41-42 の "next to the code" は文字通り成立するため矛盾が検知不能
- 現状: skill_deltas :372-401（SQLite を含まない join/order）、TwoWindow :342、
  SkillDelta :361、TrendWindowAggregates :350 が store.rs に住み、trend.rs:10
  が store 経由で import（:150 と :296 で呼び出し）。core/findings.rs の
  store-free FindingsInput が house style
- 新所見: テーブル経路（trend_skill_rows）の並び順を直接 assert するテストが
  0 件（grep 実測）、tests/ に store.rs の tripwire 無し（cli/mod.rs のみ）

## BDD受け入れシナリオ

```gherkin
Scenario: placement rule が spec に宿る
  Given .claude/rules/ に placement rule がある
  When 新しい純粋 join（SQLite を含まない row 加工）を書く
  Then rule は core への配置を要求し、2 件目発火義務を明記する

Scenario: 2 件目の純粋 join が発火する
  Given store.rs に 2 件目の SQLite を含まない純粋関数が現れる
  When PBI を立てる
  Then 同じ PR で core への移動を義務づける（trigger 乗車）
```

## 受け入れ基準
- [ ] docs/specs/architecture.md:99 の core 列挙に trend join 加筆する
- [ ] `.claude/rules/` に placement rule（fetch 済み row の加工は core へ、
  SQLite を含まない関数が store に現れたら同じ PR で core へ移動 ― 2 件目
  発火義務）を追加する
- [ ] `.claude/rules/spec-sync.md` map に 1 行追加する
- [ ] refactor（skill_deltas + TwoWindow/SkillDelta の core 移動）は trigger
  乗車（次の trend metric/view 着地 PR に同乗）― 本 PBI のスコープ外、ledger
  に記録する
- [ ] 移動義務の trigger を検知可能にする
  （grep 可能な契約 or tests/ に軽い tripwire ― judge して記録する）
- [ ] docs-charset（.claude/rules/*.md も CP932 範囲、em dash 禁止）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- なし（docs-only）

### 統合テスト
- prose reference gate が新規約の .rs 錨を検証する（PBI 36 に依存）

### 単体テスト
- なし（docs-only。judge で軽い tripwire を足す場合はその pin）

## 実装アプローチ
- **docs + 規約整備**: 振る舞いは変わらない（spec + rule のみ）。
  refactor は trigger 乗車で後続
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=0.5日。RICE 6.0。rule のみ; refactor は trigger 乗車で marginal 12.0）

## 技術的考慮事項
- 依存関係: **PBI 36 に依存**（gate が .claude/rules を走査するようになってから
  新規約の .rs 錨を書くと gate が検証する）。PBI 37-40/42 と独立
- テスタビリティ: docs-only
- 非機能要件: プライバシー（fixture 無し）、振る舞い不変

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- docs/specs/architecture.md:96-101（所有表。core 列挙 :99 に trend join が無い）
- .claude/rules/spec-sync.md map :26/:28（store.rs 行が無い）
- src/store.rs: skill_deltas :372-401、TwoWindow :342、SkillDelta :361、
  TrendWindowAggregates :350（SQLite を含まない純粋 joins）、
  two_window_aggregation :3677-3813（query → join 接続の唯一の 1 件、store に
  残す）、query 結果型（KindRollup/TrendWindowAggregates/SkillUsage、serde
  derive :321）
- src/core.rs:4-17: core の list（移動時に 1 行足す場所）
- src/core/findings.rs: store-free FindingsInput（house style）

### 実装手順
1. architecture.md:99 の core 列挙に trend join を加筆する
2. .claude/rules/ に placement rule を追加する（2 件目発火義務込み）
3. spec-sync map に 1 行追加する
4. trigger の検知方法を judge して記録する（grep 可能な契約 or 軽い tripwire）
5. ledger に refactor（trigger 乗車）を記録する

### 落とし穴
- PBI 36 先行（gate が rules を走査するようになってから新規約の .rs 錨を書く）
- .claude/rules/*.md も docs-charset 対象（CP932 範囲、em dash 禁止）
- cli/views/ 化（PBI 32/34 確定）と core 移動を 1 commit に混ぜない
  （本 PBI では移動しない）

## Definition of Done
- [ ] 全 BDD シナリオが rule 文面として充足する
- [ ] placement rule が .claude/rules/ に着地し、spec-sync map に 1 行追加
- [ ] refactor の trigger 乗車が ledger に記録される
- [ ] `just check`（直接実行）と `cargo test` が green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] architecture.md、.claude/rules/ 2 ファイル更新済み
