# PBI: trend の導出される previous 窓も暦レンジ外で拒む（--last 倍増と太古 --since の解決不能のエラー化）

## ユーザーストーリー
分析者として、previous 窓が暦レンジ外になる trend 入力を黙って受け入れずエラーで
教えてほしい、なぜなら PBI 29 が単窓の解決不能をエラー化したのに、同じ入力が
trend では空の since を描く非対称は Reporting honesty を満たさないからだ。

## ビジネス価値
- PBI 29 で閉じた「解決不能な入力が成功して空の since を描く」が、trend の
  previous 側ではまだ起きる。入口 1 箇所の対称拡張で閉じる
- 測定方法: 到達帯 [48,243,025, 96,486,050) 日の `--last` と太古の `--since`
  （clap equals 形）で trend が非ゼロ終了し、stderr に解決不能の旨が出ること。
  開示行・JSON に空の previous since が現れないこと（2026-10-31 のなぜなぜ
  分析 round2 で実バイナリ probe 実測済みの現象が消える）

## 背景（2026-10-31 のなぜなぜ分析 round2、3 立場 x 30 段階、probe 実証）
- PBI 29（コミット 5fb8184）は `resolve` の `--last` 分岐で current 窓の両端
  （start と now）を検証した。しかし `resolve_pair` が導出する previous 窓の
  起点 `previous_start`（src/core/period.rs:140-142 付近）は i64 の
  `checked_sub` ガードしか持たない
- probe 実測（実クロック、空 /tmp DB、exit=0）: `trend --last 50000000d` は
  stderr に「previous since , until -130059-...」を描き、JSON は
  `"window.previous.since": ""`。最初の暦外 N は正確に 48,243,025d
  （48,243,024d は暦内描画）、現行拒否開始は 96,486,050d
- **到達可能な --since 経路（持ち越し Context の修正発見）**:
  `trend --since=-200000-01-01`（clap equals 形。空白形は clap が '-2' で拒む）
  は exit=0 で「previous since , until -200001-12-31T23:59:59+09:00」を描く。
  以前の「--since 経路は実クロックで到達不能」は誤りで、副修正に繰り込む
- イベント集計は `started_epoch` 整数バインドで正しく、sessions 軸は TEXT
  バインド（src/store.rs:629-633 付近の unwrap_or_default）で
  `started_at >= ''` 全通過 + 太古上限の辞書順排除。影響は開示のみ
- 根本原因（なぜなぜ分析の結論）: (1) 暦検証が構築サイト（resolve --last
  分岐）に属し型・経路に属さない（WindowPair は不変条件なし、resolve_pair が
  導出する previous は unchecked）(2) 消費側が総関数で沈黙崩壊（bounds の
  map_or_else(String::new,…)、epoch_to_rfc3339_utc の unwrap_or_default）
  (3) スコープ運用がサイトスコープでパススコープでない（PBI 29 がペア側除外を
  完了として制度化）(4) spec に「導出窓」の語彙がない

## BDD受け入れシナリオ

```gherkin
Scenario: previous 窓が暦外になる --last はエラーになる
  When ユーザーが `saniwalens trend --last 50000000d` を実行する
  Then コマンドは解決できない span の旨を stderr に出力し、非ゼロで終了する
  And 開示行・JSON のどこにも空の previous since が現れない

Scenario: 境界直下の --last は通る
  When ユーザーが previous がちょうど暦内に収まる `--last` を実行する
  Then コマンドは成功し、previous の開示に空文字は現れない

Scenario: 太古の --since はエラーになる
  When ユーザーが `saniwalens trend --since=-200000-01-01` を実行する
  Then コマンドは --since を名指す解決不能エラーで非ゼロ終了する
  And invalid --last の文言は使わない（日付由来の入力に嘘をつけない）

Scenario: 通常の trend は何も変わらない
  Given 慣例どおりの使い方（7d 相当のペア、無指定）
  When ユーザーが trend を実行する
  Then 出力は従来とバイト同一で、エラーにはならない
```

## 受け入れ基準
- [ ] `resolve_pair` の `--last` 分岐で `previous_start` の `checked_sub`
  チェーンに `.filter(|p| in_calendar(*p))` を加え、同一の
  `invalid_span_error` で拒む（1 行の対称拡張、chokepoint 維持）
- [ ] `previous.end` の冗長検査は加えない（current 側で推移的に暦内に収まる。
  足すなら defensive である旨を PBI の判断として明記）
- [ ] タイリング不変 `previous.end == current.start - 1`
  （src/core/period.rs:464 付近）と +1 シフト（:136 付近）を壊さない
- [ ] `--since` 経路の太古入力は、`--since` を名指す empty-window 系エラーで
  拒む（invalid --last の文言を使わない。日付由来の入力に嘘をつけない）
- [ ] 性質テスト `every_accepted_window_renders_non_empty_bounds`
  （src/core/period.rs:617-642 付近、現状は例ベースで Ok 腕が小窓のみ）を
  now 注入の両暦端スイープに拡張し、offset 描画込みで非空 bounds を pin する
  （UTC 対 tz のエッジの観測面も同時に閉じる）
- [ ] spec 3 箇所更新: `docs/specs/cli.md` の期間解釈節（導出される previous
  窓の語彙、192-194 行目付近）、Reporting honesty（導出窓を含める、256-262
  行目付近）、Views 表（tiling 行、134 行目付近の「Both windows' ranges are
  disclosed」が導出窓を含む旨）
- [ ] テスト: `resolve_pair(None, None, Some("50000000d"))` が Err（現状 Ok で
  Red から）、now 注入での境界 pin（PBI 29 の edges テストのペア版）、
  `--since=-200000-01-01` がエラー（必ず equals 形）、E2E は
  `tests/period_errors.rs` に隣接追加
- [ ] `just check` と `cargo test` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `trend --last 50000000d` と `trend --since=-200000-01-01` が stderr + 非ゼロ

### 統合テスト
- `resolve_pair` の previous 暦外エラー化と境界値（both-ends のペア版）

### 単体テスト
- 性質テストのスイープ拡張（両暦端の now 注入で非空 bounds）
- 既存 30 期間系テストの回帰（無変更を pin）

## 実装アプローチ
- **Outside-In**: E2E（エラーパス）から失敗を確認し、`resolve_pair` の
  previous 検証（統合）→ 性質テストのスイープ（単体）の順に Red-Green-Refactor
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0: R=2 / I=2 / C=3 / E=1日。前例 PBI 29 と同形・同点）

## 技術的考慮事項
- 依存関係: PBI 29（5fb8184）が前提（`in_calendar` と
  `invalid_span_error` が実装済み）。PBI 34/35 と独立
- テスタビリティ: `resolve_pair` は純粋関数（now 注入）。E2E は合成 fixture
- 非機能要件: プライバシー（合成 fixture のみ）、通常 trend の出力バイト同一

## 実装者向け注記

### 現状コードの確認（2026-10-31 なぜなぜ分析で実証済み）
- `src/core/period.rs` の `resolve_pair`（--last 分岐）: current を
  `resolve` に通して検証済み、`previous_start`（140-142 行目付近）は
  `checked_sub` のみ。ここに `.filter(|p| in_calendar(*p))` を組みむ
- `--since` 経路（173 行目付近、`start - length`）: 無検証。到達可能実証済み
  （equals 形）
- `bounds`（241-251 行目付近）と `epoch_to_rfc3339_utc`
  （src/store.rs:629-633 行目付近）は直さない。入口で拒めば下流に暦外は
  届かなくなる
- PBI 29 のテスト（tests/period_errors.rs、src/core/period.rs の edges
  テストと定数 pin）が隣接にあり、同じ型で広げる

### 実装手順
1. Red: `resolve_pair(None, None, Some("50000000d"))` が Ok を返す現状を
   pin するテストを書く（期待は Err）
2. `resolve_pair` の `previous_start` に `in_calendar` フィルタを加える
3. `--since` 経路の太古入力を `--since` 名指しのエラーで拒む
4. 性質テストを now 注入の両暦端スイープに拡張する
5. E2E を tests/period_errors.rs に隣接追加する
6. `docs/specs/cli.md` 3 箇所（期間解釈・Reporting honesty・Views 表）を
   更新する
7. `cargo test` → `just check`（直接実行）→ 通常 trend の出力不変を確認

### 落とし穴
- `--since` のテストは必ず **clap equals 形**（`--since=-200000-01-01`）で
  書く。空白形は clap が '-2' で拒むため別のバグになり、暦検証のテストに
  ならない
- エラー文言を新設しない。`--last` 由来は PBI 29 と同一の invalid-span、
  `--since` 由来は既存の empty-window 系で `--since` を名指す
- フィルタは `checked_sub` チェーンの内側に組みむ（外側の別検証にすると
  タイリング不変と +1 シフトの経路が分かれる）
- `store.rs` / `src/cli/{trend,window}.rs` / 単窓 8 ビューには触れない
  （入口で既に守られている）

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとして実装されパスする
- [ ] `just check` と `cargo test` が green（既存期間系テスト含む）
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] docs/specs/cli.md 更新済み（期間解釈 + Reporting honesty + Views 表）
