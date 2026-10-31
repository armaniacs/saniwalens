# PBI: 窓付き flag surface を 1 モジュールに集約する（9 重複製 → 1 宣言済み引数群 + 1 窓付きセッション open 呼び出し）

## ユーザーストーリー
実装者として、窓付き flag surface（6 フィールド + conflicts_with_all + 順序付き
配線）に 1 つの interface が割り当てられていてほしい、なぜなら 9 variant x 9 arm
に複製されていて、順序不変条件が arm ごとのコメントにしか宿らないからだ。

## 優先度
- 順位: 40 / 全候補数 7
- RICEスコア: 6.0（Reach=2 / Impact=2 / Confidence=3 / Effort=2日）
- 根拠: 欠陥未発生・複製防止が本丸（暦系 fix の 12.0 より緊急性低）。
  PBI 32 と同水準。PBI 38 と同じ cli/mod.rs 触りのため 38 の後に直列

## ビジネス価値
- 順序不変条件「flags resolve before the store opens」が module で 1 回 pin され、
  spec の 1 文（cli.md:72-75, 204-208）が実装の 1 対応物を得る
- 測定方法: mod.rs が約 200-350 行縮み（tripwire green のまま）、9 subcommand の
  --help capture diff が空であること

## 背景（なぜなぜ分析 2026-11-01 round3、3 立場 x 30 段階、file:line 実証）
- 窓付き flag surface に 1 つの interface が割り当てられず、9 variant x 9 arm に
  手続きとして複製されている（mod.rs:85-322 宣言 x9 約170 行、:391-585 配線 x9）
- session.rs の seam（PBI 01）は「解決系 = harness + store path + opened
  store」で止まり窓がその外（session.rs:87-91, 135-151）のため、順序不変条件は
  arm 手続き順とコメント（mod.rs:380-384, :425-427）にしか宿らない
- pin が PBI 着地単位で散り conflict 3/9（mod.rs:3313-3382）、順序 2/9
  （period_errors.rs ― しかも assert は arm 並べ替えでも成立する: freshness 行
  session.rs:190-195 と store 副作用が pin の外）
- tripwire は mod.rs=3,383 行=上限そのもの（cli_mod_line_budget.rs:16）で
  「停止」のみ構造保証し「収斂」を保証せず、Command variant/run() arm は
  views/ に置けないため（architecture.md:109-110）新窓 view は必ず budget
  引き上げを強制する
- PBI 29/33 は period.rs+cli.md+period_errors.rs の 3 ファイルのみに着地し、
  9 arm を検査対象から制度化して外した（PBI 33:145）― arm 複製は凍結されたまま
  次の窓 flag 変更を危くする

## BDD受け入れシナリオ

```gherkin
Scenario: 窓付き flag は 1 回だけ宣言される
  Given 1 つの宣言済み引数群がある
  When 9 窓付き command がそれを使う
  Then flag 宣言は 1 箇所で、conflicts_with_all も 1 箇所で pin される

Scenario: 順序不変条件が module で pin される
  Given 新 module が parse_format → [parse_scope] → parse_period/parse_pair
  → Session::open_read の同順序で配線する
  When arm の並べ替えを試す
  Then pin が失敗する（順序は module の、コメントではない）

Scenario: 出力と help はバイト同一である
  Given capture で検証済みの出力と --help がある
  When 本 PBI を着地させる
  Then 9 subcommand の --help capture diff が空で、エラー順序込みでバイト同一
```

## 受け入れ基準
- [x] 9 複製 flag 宣言を 1 宣言済み引数群に統合する
  （clap flatten + conflicts_with_all の arg id を最初に実証）
- [x] arm ごとの順序付き配線を 1 窓付きセッション open 呼び出しに集約する
  （parse_format → [parse_scope] → parse_period/parse_pair → Session::open_read
  の同順序を新 module 内で pin。mod.rs:391-585 の同順序）
- [x] mod.rs を約 200-350 行縮める（tripwire green のまま、budget は同一 PR
  で再測定・更新する ― PBI 38 規範: budget 変更は spec update と同一 PR、
  測定サイズ zero-slack。PBI 38 が 3,383 から 3,322 に再測定済みなので、
  本 PBI の縮小後は 3,322 から再測定する）
- [x] help 文面バイト同一 + trend pair 固有文面: 8 view は逐語同一 doc、trend の
  3 旗は pair 固有（mod.rs:114-125）― 移動前後で 9 subcommand の --help
  capture diff を空に
- [x] 順序不変条件の pin（PBI 01 バイト同一 + tests/period_errors.rs。
  frozen 経路 freshness（session.rs:143-147）と optimize arm 例外順序
  （:600-606）には触れない）
- [x] view-landing rule 再開禁止（views/ を作らない、trend.rs も移動しない、
  テストは移動先 mod に従属 ― tdd.md:41）
- [x] spec 同一変更: cli.md:72-75, 204-208（順序不変条件が 1 対応物を得る）+
  architecture.md:101 層表 report 行
- [x] conflict 属性落ちの 6 view（period.rs:47-52 の core 第二防衛が捕めるが
  clap 系と文言・終了コードが別系統、cli.md:187-188 は「clap conflict」と明記）
  を機械化するか判断して記録する

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 9 subcommand の --help capture diff 空 + PBI 01 バイト同一（エラー順序込み）

### 統合テスト
- 新 module の順序 pin（parse_format → scope → period → open_read）

### 単体テスト
- conflicts_with_all の arg id pin（flatten 動作の実証）

## 実装アプローチ
- **Outside-In**: --help capture diff 空 + バイト同一の確認から、1 宣言済み
  引数群 → 1 open 呼び出しの順
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=2日。RICE 6.0。PBI 32 と同水準）

## 技術的考慮事項
- 依存関係: PBI 38 と同じ cli/mod.rs 触りのため 38 の後に直列。
  PBI 36/37/39/41/42 と独立
- テスタビリティ: 新 module は純粋配線、順序 pin 可能
- 非機能要件: プライバシー（fixture は合成）、出力 byte-identity

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- src/cli/mod.rs: Command enum :67-364（9/11 が 6 flag を再宣言 :85-322）、
  run() :366-609（9 arm 配線 :391-585、コメント :380-384/:425-427）、trend の
  3 旗 :114-125（pair 固有文面）、conflict pin :3313-3382（3/9）、whole-history
  契約 pin :3280-3311、optimize arm 例外順序 :600-606
- src/cli/window.rs: parse_period/parse_pair :15-37
- src/session.rs: seam :87-91, 135-151（窓の外で止まった現状）、freshness
  :143-147、:190-195
- tripwire: tests/cli_mod_line_budget.rs:17（PBI 38 が 3,383 から 3,322 に
  再測定済み、zero-slack。本 PBI の縮小後に再測定する）

### 実装手順
1. clap flatten + conflicts_with_all の arg id を最初に実証する
2. 1 宣言済み引数群に統合する（8 view 逐語同一 doc、trend の 3 旗は pair 固有）
3. 1 窓付きセッション open 呼び出しに集約し、順序を新 module 内で pin する
4. --help capture diff 空 + PBI 01 バイト同一を確認する
5. spec 同一変更（cli.md、architecture.md）
6. 検証（tripwire green、budget は縮小後に同一 PR で再測定・更新）

### 落とし穴
- PBI 29/33 は 9 arm を検査対象から制度化して外した ― arm 複製は凍結されたまま
  次の窓 flag 変更を危くする（本 PBI がそれを集約する）
- 順序の並べ替えは既存 period_errors.rs をすり抜ける（freshness 行と store
  副作用が pin の外）― 新 pin は freshness 行込みで
- budget は縮小後に同一 PR で再測定・更新する（PBI 38 規範、zero-slack。
  PBI 38 が 3,383 から 3,322 に再測定済み）
- docs-charset: em dash U+2014 不可、.claude/rules/*.md も対象

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] 9 subcommand の --help capture diff が空、出力はバイト同一
- [x] mod.rs が約 200-350 行縮み（tripwire green、budget 変更なし）
- [x] `just check`（直接実行）と `cargo test` が green
- [x] コードレビュー完了（task review + 最終 review）
- [x] cli.md、architecture.md 更新済み
