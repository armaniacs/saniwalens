# PBI: 極端な --last を暦レンジ外で拒む（解決不能な入力のエラー化）

## ユーザーストーリー
分析者として、解決できない `--last` を黙って受け入れずエラーで教えてほしい、
なぜなら「空の since」や暦外の日付は誤読を生み、Reporting honesty の水準を
満たさないからだ。

## ビジネス価値
- 解決不能な入力が「成功したが意味のない出力」にならず、毎回エラーで分かる
- 測定方法: 暦外 span で 8 ビューと trend が非ゼロ終了し、stderr に
  invalid-span の旨が出ること。開示行・JSON・store のバインドに空文字が
  現れないこと（2026-10-31 のなぜなぜ分析で probe 実測済みの現象が消える）

## 背景（2026-10-31 のレビュー持ち越し指摘2、なぜなぜ分析の実証）
- chrono 0.4.45 では暦外 epoch で `timestamp_opt` が None を返すため、
  `bounds` の `map_or_else(String::new, ...)`（src/core/period.rs:219-225）で
  「年 -252,700 の日付」ではなく**空の since** が表示される（probe 実測:
  `report window: since , until 2026-10-31T...`、JSON `{"since":""}`）
- 暦内でも荒唐無稽な日付は起きる: `--last 70000000d` は `since -189627-05-05`
- trend の `checked_sub` ガード（src/core/period.rs:127-132）は倍増の i64
  算術だけを守り、`--last 50000000000000d` は trend も受け入れて
  current.start が暦外になる（probe 実測）
- store 側も `epoch_to_rfc3339_utc` の `unwrap_or_default`
  （src/store.rs:627-631）で `sessions.started_at >= ''` に崩れ、辞書順で
  全セッションが通る沈黙崩壊になる
- 暦外系のテストは 0 件（cargo test period = 25 passed、倍増のテストのみ）

## BDD受け入れシナリオ

```gherkin
Scenario: 暦外の --last はエラーになる
  Given saniwalens が使える
  When ユーザーが `saniwalens usage --last 92233720368547d` を実行する
  Then コマンドは解決できない span の旨を stderr に出力し、非ゼロで終了する
  And 表・JSON・freshness 行のどこにも空の since が現れない

Scenario: 暦内の span でも解決後の窓が暦外になるならエラーになる
  When ユーザーが `saniwalens usage --last 100000000d` を実行する
  Then コマンドは同じ invalid-span エラーで非ゼロ終了する
  And 開示行に暦に存在しない日付は現れない

Scenario: trend も同じ入口で拒まれる
  When ユーザーが `saniwalens trend --last 50000000000000d` を実行する
  Then コマンドは同じ invalid-span エラーで非ゼロ終了する

Scenario: 通常の --last と無指定は何も変わらない
  Given 慣例どおりの使い方（7d / 24h / 2w、無指定）
  When ユーザーが各ビューを実行する
  Then 出力は従来とバイト同一で、エラーにはならない
```

## 受け入れ基準
- [x] `resolve` の `--last` 分岐（src/core/period.rs:53-58）で、解決後の窓の
  両端が chrono の暦レンジ（NaiveDate が表現できるおよそ262,143年、
  epoch でおよそ -8,334,601,228,800 から +8,210,268,767,999）に収まることを
  検証し、外れる場合は既存の `invalid_span_error`
  （src/core/period.rs:192-196）を返す
- [x] 入口 1 箇所の修正で、8 ビュー + trend（current 側）+ window_json +
  store の沈黙崩壊が同時に閉じること（呼び出し側の個別修正をしない）
- [x] `resolve_pair` の倍増ガード（checked_sub）は残す。スコープは「単窓
  8 ビュー + 描画 / json / store の暦外経路」に限定する
- [x] spec 更新: `docs/specs/cli.md` Period 節に `--last` の値域契約を追記し、
  Reporting honesty に「入力由来の解決不能はエラー」を追記する
- [x] テスト: 暦外 span（92233720368547d、100000000d）の resolve エラー化、
  trend current 側（50000000000000d）、開示行の since/until が非空である
  性質テスト、通常 `--last` と無指定の回帰ピン止め
- [x] `just check`（rustfmt + clippy）と `cargo test` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `usage --last 92233720368547d` が stderr + 非ゼロで終わり、stdout が空

### 統合テスト
- `resolve` / `resolve_pair` の暦レンジ検証（境界値、暦内最大の正常 span）

### 単体テスト
- span と暦レンジの境界判定（`NaiveDateTime::MIN` / `MAX` の timestamp 使い）
- 既存 25 テストの回帰（無変更を pin）

## 実装アプローチ
- **Outside-In**: E2E（エラーパス）から失敗を確認し、`resolve` の入口検証
  （単体）→ 呼び出し経路の回帰（統合）の順に Red-Green-Refactor
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0: R=2 / I=2 / C=3 / E=1日）

## 技術的考慮事項
- 依存関係: なし（trend の倍増ガードは実装済み、残す）
- テスタビリティ: `resolve` は純粋関数（now 注入）。境界は合成 fixture
- 非機能要件: プライバシー（合成 fixture のみ）、既存出力のバイト同一

## 実装者向け注記

### 現状コードの確認（2026-10-31 実施済み）
- `src/core/period.rs` の `resolve`（40-76行目付近）の `--last` 分岐:
  現状は `now - secs` を検証なしで返す。`now > 0` のため i64 は溢れないが、
  暦レンジの検証が無い
- `invalid_span_error`（192-196行目付近）をそのまま使い、新しいエラー文言は
  作らない（既存の invalid --last と同一文言で足りる）
- `bounds`（217-227行目付近）と `epoch_to_rfc3339_utc`
  （src/store.rs:627-631行目付近）は直さない。入口で拒めば下流に暦外は
  届かなくなる

### 実装手順
1. `src/core/period.rs` に暦レンジの定数（`NaiveDateTime::MIN` /
   `MAX` の timestamp）を定義する
2. `resolve` の `--last` 分岐で `now - secs` が暦レンジ内であることを
   `ok_or_else(invalid_span_error)` で検証する
3. テストを追加（暦外 span の resolve エラー化、trend current 側、
   性質テスト、回帰ピン止め）
4. `docs/specs/cli.md` の Period 節と Reporting honesty を更新する
5. `cargo test` → `just check`（直接実行）→ 出力不変を確認

### 落とし穴
- 暦レンジの上限側: `day_end_epoch` が +86,400-1 するため、MAX 日付の
  端数で境界を越えないよう、検証は窓の両端（start と end）それぞれで行う
- 「空の since」は現状のテストで pin されていない。先に現状の振る舞いを
  見るテスト（Red）を書いてから直すと回帰に強い
- エラー文言を新設しない。invalid-span の既存文言に統一すること（下流の
  エラーハンドリングと spec の Errors 節が1語のまま保てる）

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] `just check` と `cargo test` が green（既存 25 期間系テスト含む）
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/cli.md 更新済み（Period 節 + Reporting honesty）
