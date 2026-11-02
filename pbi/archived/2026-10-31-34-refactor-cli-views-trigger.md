# PBI: cli/views 化のトリガーを構造で保証する（mod.rs 上限 tripwire と発火 PR 単位の規範）

## ユーザーストーリー
実装者として、「次の view が着地するときに cli/views/ を作る」という先送り判断
のトリガーが prose でなく構造で保証されていてほしい、なぜなら成立条件が観測点
ゼロの prose に託されていると、発火時に同じ判断が繰り返されて mod.rs が再肥大
するからだ。

## ビジネス価値
- PBI 32 の「待つ」判断を撤回せず、撤回不要な状態に構造で到達する
- 測定方法: mod.rs が上限を超える追加を含む変更で tripwire が失敗し、メッセージ
  が views/ を指すこと。現状の HEAD では tripwire が通ること（2026-10-31 の
  なぜなぜ分析 round2 で 3 立場 x 30 段階、実測付きで検証済みの判断）

## 背景（2026-10-31 のなぜなぜ分析 round2、3 立場 x 30 段階、実測）
- PBI 32 で `src/cli.rs`（4,570 行）を cli/mod.rs + trend.rs（1,101 行）+
  window.rs（107 行）+ report.rs（98 行）に分割した。残る mod.rs は 3,383 行
  （12 サブコマンド + 8 report view + delivery seam 219 行 + doctor 部 469 行
  + tests 612 行 / 25 件）
- 前回のなぜなぜ分析が恐れた「複利」は PBI 32 の抽出で構造的に閉じている:
  mod.rs の成長ベクトルは (a) 新 view の誤追記（防げる唯一のベクトル）
  (b) shell 機能の正当追加 (c) trend/window/report 系の成長（既に各ファイルに
  閉じ込め済み）の 3 本。trend.rs の移動コストは移動タイミングに非依存
- 先送り判断の成立条件 4 つがすべて prose に託され観測点 0:
  (1) 発火観測（prose のみ、tripwire 無し、.claude/rules に規約無し）
  (2) views/ 作成時の trend.rs 同コミット移動義務（architecture.md:101 本文に
  は書かれていない、git log -L で確認）(3) 発火時 spec 更新マップ（:101 の
  2 箇所）(4) 移動検証の golden
- 新所見: mod.rs に残るヘルパはほぼ全て view 専用（fmt_tokens / plural /
  prune_phrase / tilde_path / shorten_to_tilde は doctor 専用、savings_* は
  waste 専用）で「共有」の名目と実態が乖離。さらに全 view の JSON 機械契約
  （docs/specs/cli.md:134 が "the machine contract" と明記）を固定する golden
  が 0 件（tests/ は trend の markdown/table 3+2 と period_errors のみ）
- 先頭 doc が「thin shell」と自称する drift が fresh implementer を誤追記に
  誘導する

## BDD受け入れシナリオ

```gherkin
Scenario: mod.rs が上限を超えると tripwire が失敗する
  Given mod.rs に上限を超える追加が含まれる変更がある
  When cargo test を実行する
  Then tripwire が失敗し、メッセージが cli/views/ を指す

Scenario: 現状の HEAD では tripwire が通る
  When cargo test を実行する
  Then tripwire は成功する

Scenario: view 追加の PR は views/ を作る
  Given architecture.md の発火 PR 単位の行動規範がある
  When Command variant を追加する PR を読む
  Then 規範は同一 PR での cli/views/ 作成と cli/trend.rs の同コミット移動と
  mod.rs への view 関数追記の禁止を要求する
```

## 受け入れ基準
- [x] `docs/specs/architecture.md:101` 付近を発火 PR 単位の行動規範に強化する
  （Command variant を追加する PR は同一 PR で `src/cli/views/` を作成、
  `src/cli/trend.rs` を同コミットで移動、view を 1 ファイルで着地、view 関数
  は mod.rs に追記しない）+ 発火時 spec 更新マップ（層表と移行規約の 2 箇所）
- [x] mod.rs 上限 tripwire テストを追加する
  （`include_str!("mod.rs")` 由来の行数、上限は現実測の 3,383 をそのまま、
  超過メッセージが views/ を指す。余裕を持たせない）
- [x] `.claude/rules/` に view 追加 PBI の受け入れ基準テンプレート 1 節を
  追加する（views/ 作成 + trend 同コミット移動 + mod.rs 追記なし +
  spec 同時更新 + golden 規則）
- [x] view 専用ヘルパ（fmt_tokens / plural / prune_phrase / tilde_path /
  shorten_to_tilde、savings_*）の doc comment に専用マーカーを付ける
  （「共有」の名目と実態の乖離を名指しする）
- [x] 待つ判断の撤回条件を PBI / 台帳の運用メモに明記する
  （tripwire bump で shell 部が +300 行成長したら views/ 化を前倒しする）
- [x] `just check` と `cargo test` が green（tripwire は通る状態で着地）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- tripwire のメッセージ文言（views/ を指す）は単体で pin

### 統合テスト
- cargo test フルスイート green（tripwire 通過）

### 単体テスト
- mod.rs 行数カウントの検証（include_str! の改行数と wc -l の一致）

## 実装アプローチ
- **リファクタリング + 規約整備**: 振る舞いは変わらない（テストと doc と規約
  のみ）。tripwire を Red にせず、現状で Green の着地とする
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=0.5日。RICE 12.0: R=1 / I=2 / C=3 / E=0.5日。比較: 今 views/ を作る
= RICE 6.0（半分移行状態で層表の 2 段階書き換えを迫られる）、8 view 含め全部
移す = RICE 2.0（JSON golden 0 件で移動検証が capture 頼み））

## 技術的考慮事項
- 依存関係: PBI 32（完了済み）が前提。PBI 33 と独立、PBI 35（参照整合 gate）と
  独立だが architecture.md に触れるため docs 系は直列
- テスタビリティ: tripwire は cargo test、純粋（include_str!）
- 非機能要件: プライバシー（fixture 無し、テストは行数のみ）、出力不変

## 実装者向け注記

### 現状コードの確認（2026-10-31 なぜなぜ分析で実測済み）
- `src/cli/mod.rs`: 3,383 行（HEAD `37a3977` 実測）。tripwire の上限はこの値
  をそのまま使う（余裕を持たせない。超過時の引き上げがレビュー点になる設計）
- `src/cli/trend.rs`: 1,101 行。移動コストはタイミングに非依存
- `docs/specs/architecture.md:101` 付近: 移行規約の現行文（発火 PR 単位の
  行動規範への強化先）
- view 専用ヘルパの所在: fmt_tokens / plural / prune_phrase / tilde_path /
  shorten_to_tilde（doctor 系）、savings_*（waste 系）。実ファイルで呼び出し
  元を確認してからマーカーを付ける

### 実装手順
1. tripwire テストを追加する（src/cli/mod.rs のテスト群または tests/。現在の
   行数を pin、超過メッセージは views/ を指す）
2. `docs/specs/architecture.md` の移行規約を発火 PR 単位の行動規範に強化する
3. `.claude/rules/` に受け入れ基準テンプレート 1 節を追加する
4. view 専用ヘルパの doc comment にマーカーを付ける
5. 台帳の運用メモに撤回条件を記録する
6. `cargo test` → `just check`（直接実行）→ tripwire 通過を確認

### 落とし穴
- tripwire の上限に余裕を持たせない（+100 行の余裕は誤追記を許す）。
  超過時の引き上げは spec 更新と同じ PR で行う（引き上げの瞬間がレビュー点）
- 行数カウントは改行数で pin する（表示幅でなく）。include_str! の文字列から
  数える場合と wc -l の差（最終行の改行）を確認する
- architecture.md の強化で「views/ は次の view で作る」の既存文を消さない。
  発火 PR 単位の規範に書き換える（撤回ではなく強化）
- `.claude/rules/` の .md も docs-charset の対象（CP932 範囲、em dash 禁止）

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] tripwire が現状 green で着地し、メッセージが views/ を指す
- [x] `just check` と `cargo test` が green
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/architecture.md、.claude/rules/、台帳の運用メモ更新済み
