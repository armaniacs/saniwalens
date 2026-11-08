# PBI: RunLineRenderer 汎化 + guide 重複削除

## ユーザーストーリー
開発者として、RunLineRenderer が opencode 型に依存せず汎用的に定義され、guide の harness 表記重複が解消されてほしい。なぜなら Kilocode 追加以降の harness 分岐コピペが 14 arm に増殖し、次の harness 追加コストと表示ずれリスクが上がっているからだ。本 PBI は振る舞い保存リファクタであり、ユーザー可視の出力は変えない。

## 優先度
- 順位: 5 / 候補 C2 (C1 の後)
- RICE: 3.0 (Reach=3 / Impact=2 / Confidence=100% / Effort=2)
- 根拠: 機能追加ではなく負債圧縮。C1 (本体機能) の後に実施し、以降の harness 拡張を安くする。

## 背景 (現状コード)
- `src/adapter/harness.rs:25` : `pub type RunLineRenderer = fn(&str) -> opencode::RunEvent;` が opencode 型を公開型に漏らしている
- `src/adapter/harness.rs:411-416` : Kilocode が `opencode::render_run_line` にマップされ、汎用シグネチャの裏付けが opencode 依存になっている
- `src/adapter/harness.rs:203-223` : catalog 分岐、`232-238` : analyze 分岐、`258-454` : 14 のコピー arm
- `src/session.rs:74-80` : `parse_harness` のエラーメッセージが harness 一覧をハードコード (`claude | opencode | kilocode`)
- `src/cli/views/guide.rs:95-101` : `harness_name()` が `label()` 系表示と重複
- `src/cli/views/guide.rs:112-115` : `equivalent_command` が Claude だけ特殊扱い (空文字 arm)

## BDD受け入れシナリオ

```gherkin
Scenario: kilocode の headless 描画が opencode 型名なしで動く
  Given kilocode 設定のストアと headless 実行ストリームがある
  When  headless ループが RunLineRenderer 経由で行を描画する
  Then  描画結果は変更前と同一になる
  And   呼び出し側のコードに opencode:: という型名が出ない

Scenario: guide の harness 表記がバイト同一のまま重複だけ消える
  Given 3 harness (claude / opencode / kilocode) x 全 Purpose x 全 ScopeChoice の表がある
  When  harness_name と equivalent_command の重複分岐を整理する
  Then  全組み合わせの guide 出力文字列が変更前とバイト同一になる
```

## 受け入れ基準
- [ ] `RunLineRenderer` が `opencode::` 型に依存しない定義になり、呼び出し側が opencode 型名を書かずに済む
- [ ] Kilocode の headless 描画結果が変更前と同一 (行単位比較で差分なし)
- [ ] `guide` の harness 名表示と `equivalent_command` 出力が全組み合わせでバイト同一
- [ ] `equivalent_command` の Claude 特殊扱い arm が消え、`harness_name()` 単一起点になる
- [ ] catalog / analyze 分岐のコピー arm が減り、追加 harness が 1 箇所の追加で済む形になる
- [ ] `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test` が green

## テスト戦略 (t-wada式 Outside-In)
- 統合: kilocode headless ストリームを行単位で描画し、変更前出力と diff して同一を確認 (RED: 新シグネチャ未導入では型名参照が残る)
- 単体: `harness_name()` と `equivalent_command()` の全組み合わせ表テスト (既存 table test を起点にし、期待値を変えずにリファクタ)
- 単体: `parse_harness` の未知名エラー文が 3 harness を列挙すること、欠測時 Claude 既定が保たれること
- 回帰: `cargo test` 全件 green をもって catalog / analyze の振る舞い同一を担保する

## 見積もり
2日 (RICE の Effort=2 と同一根拠。小規模リファクタ + 表テスト拡充分)

## 実装者向け注記
- 方針: 振る舞い保存。出力文字列・描画結果を一切変えない。まず表テストで現出力を固定してから型を汎化する
- 手順案:
  1. guide の全組み合わせ期待値を固定 (既存 table test を確認・不足分だけ追加)
  2. `harness_name()` を単一起点にし、`equivalent_command:112-115` の Claude 空文字 arm を `harness_name()` 利用に置換 (出力 diff なしを確認)
  3. `RunLineRenderer:25` の戻り型を harness 共通イベント型 (またはジェネリクス/トレイト) に汎化し、`411-416` の kilocode arm が opencode 名を経由しない形にする
  4. catalog (`203-223`) / analyze (`232-238`) / 14 arm (`258-454`) の重複を共通化 (マップ表 or ヘルパー化)。`session.rs:74-80` の一覧文字列も `Harness` 側の単一起点から生成する
- 落とし穴:
  - `fn(&str) -> X` の型エイリアス変更は呼び出し側全件に波及する。`grep -rn "RunLineRenderer\|render_run_line" src/` で事前に影響範囲を確定する
  - Claude は headless ストリームを持たない (`None` arm)。汎化後も `None` を保つこと
  - spec 変更は不要見込み (表示契約の変更なし)。変えた場合は `docs/specs/harness.md` を同変更で更新する

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとして実装されパスする
- [ ] 受け入れ基準の 6 項目を満たす
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み (spec 変更なしなら不要と明記)

## 検証記録 (B3統合・2026-11-09)

- 実装: harness-local `RunLine` enum＋`From<opencode::RunEvent>`、public signatureから `opencode::` 排除、adapter層内に1箇所集約。`catalog_surfaces` 単一match化、`Harness::ALL`＋`label_list`、`parse_harness`/`guide::harness_name` 一本化。
- 統合側残務: `headless.rs` の呼出し移行（`RunEvent::`→`RunLine::` 4箇所＋import）＋type_complexity lintに `SiteSet` 別名で対応。
- 境界検証: fmt OK、clippy 0 errors、lib 642 passed、parity 3 passed（kilo==opencodeバイト一致含む）、golden不変。
