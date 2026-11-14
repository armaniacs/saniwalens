# PBI: advisor 言語の決定根拠の表示

## ユーザーストーリー
日本語ロケールの利用者として、advisor がなぜ英語（または日本語）で
応答するのか知りたい。なぜなら意に反する言語で出たときに、どこを直せば
よいか（フラグか設定ファイルか環境変数か）分からないからだ。

## ビジネス価値
- 「なぜ英語なのか」問い合わせの自己解決 (決定ソースの一行表示)
- 測定方法: `--verbose` 実行時に言語の決定ソース
  (flag / config / locale / default のいずれか) が一行で確認できること

## 現状確認 (Phase 0 済み)
- 決定ソースを出力する経路は存在しないことを確認済み
  (`grep -rni "decision source\|decided by" src/session.rs src/cli/mod.rs` → ヒットなし)
- 解決順自体は `resolve_cli_language_prefers_flag_over_file_over_locale_over_english`
  で固定済み。表示だけが無い状態
- 関連 PBI: `pbi/2026-11-11-01-feat-locale-language-fallback.md` (解決ロジック本体)

## BDD受け入れシナリオ

```gherkin
Scenario:  verbose で決定ソースが分かる
  Given config に language="ja" があり、LANG=ja_JP.UTF-8 である
  When  --verbose 付きで実行する
  Then  言語の決定ソースが config である旨が一行で表示される

Scenario:  locale 決定であることが分かる
  Given フラグにもconfigにも言語指定がなく、LANG=ja_JP.UTF-8 である
  When  --verbose 付きで実行する
  Then  言語の決定ソースが locale である旨が一行で表示される

Scenario: 通常実行の出力は変わらない
  Given 任意の言語設定である
  When  --verbose なしで実行する
  Then  出力は従来とバイト同一である
```

## 受け入れ基準
- [x] 決定ソース (flag / config / locale / default) が `--verbose` 時のみ一行で出る
- [x] 通常出力・JSON 出力はバイト同一 (機械契約に触れない)
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green
- [x] 出力先・文言が spec (`docs/specs/cli.md`) と一致している (変更があれば同一変更で更新)

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `--verbose` 実行の出力に決定ソース行が含まれること (合成 config + 合成 locale)

### 統合テスト
- 4 ソースそれぞれの表テスト (flag 指定時 → flag、config のみ → config、locale のみ → locale、何もなし → default)

### 単体テスト
- ソース判定ロジックの境界値テスト (flag と config の両方あり → flag、config ja + LANG=C → config)

## 実装アプローチ
- **Outside-In**: E2Eテストから開始し、失敗を確認してから実装
- **Red-Green-Refactor**: TDDサイクルを各レイヤーで適用
- **リファクタリング**: グリーンになるたびに品質改善

## 見積もり
2 (要チームでの見積もり。内訳目安: ソース伝搬 1、表示+テスト 1)

## 技術的考慮事項
- 依存関係: 解決結果にソースを添える形。既存の `--verbose` 経路があればそこに乗せ、
  なければ `doctor` 出力等の既存デバッグ面を探す (新規フラグは増やさない)
- テスタビリティ: 環境変数は注入可能にし、テストは合成値のみ
- 非機能要件: 通常出力と JSON (機械契約) には一切触れない

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること）
```bash
# 決定ソース出力が無いことの再確認
grep -rni "decided by\|decision source" src/ | head
# --verbose の既存経路を探す
grep -rn "verbose" src/cli/mod.rs src/session.rs | head -n 20
# 解決関数の戻り値の形
grep -n "resolve_cli_with_language\|ResolvedCli" src/session.rs | head -n 20
```

### 実装手順
1. E2Eテストを Red で書く (`--verbose` 出力にソース行)
2. ソース判定の統合テストを Red で書く (4 ソースの表)
3. 解決結果にソースを添えて Green にする (戻り値の拡張または並行返却)
4. `--verbose` 経路に一行表示を足す (通常・JSON 出力不変を確認)
5. spec を同一変更で更新する (文言を変えた場合のみ)

### 落とし穴
- 解決関数の純粋性を崩さない (環境変数は呼び出し側で読むこと)
- JSON 出力に紛れ込ませない (機械契約の golden が壊れる)
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] テストカバレッジが基準を満たす (E2E/統合/単体すべて)
- [x] コードレビュー完了
- [x] リファクタリング完了 (グリーン後)
- [x] ドキュメント更新済み
