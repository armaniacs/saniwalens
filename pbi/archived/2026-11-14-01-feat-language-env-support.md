# PBI: `LANGUAGE` 環境変数による advisor 言語解決

## ユーザーストーリー
Ubuntu デスクトップ等の利用者として、`LANG=C.UTF-8` でも `LANGUAGE` に
日本語が入っていれば advisor を日本語で使いたい。なぜならその環境では
`LANGUAGE` が実際にものを言う言語指定だからだ。

## ビジネス価値
- GNU gettext 圏の実環境 (`LANG=C.UTF-8` + `LANGUAGE=ja_JP:ja`) でゼロコンフィグ日本語化
- 測定方法: 上記環境でフラグ・config 未指定のときプロンプトに日本語指示が
  含まれ、`LC_ALL=C` のもとでは従来どおり英語のままであること

## 現状確認 (Phase 0 済み)
- `grep -rn "LANGUAGE" src/session.rs` → ヒットなし。`resolve_locale_language`
  は `LC_ALL` > `LC_MESSAGES` > `LANG` のみを見る
- 優先順位フラグ > config > ロケール > 既定は
  `resolve_cli_language_prefers_flag_over_file_over_locale_over_english`
  で固定済み。本 PBI はロケール段の内部拡張に留まる
- 関連 PBI: `pbi/2026-11-11-01-feat-locale-language-fallback.md` (親機能・実装済み)

## BDD受け入れシナリオ

```gherkin
Scenario: LANGUAGEだけで日本語になる
  Given フラグにもconfigにも言語指定がなく、LANG=C.UTF-8 かつ LANGUAGE=ja_JP:ja である
  When  プロンプトを組み立てる
  Then  日本語応答の指示が含まれている

Scenario: CロケールはLANGUAGEに勝つ
  Given LANG=C.UTF-8 かつ LANGUAGE=ja_JP:ja かつ LC_ALL=C である
  When  プロンプトを組み立てる
  Then  日本語指示は含まれない (gettext の C 優先に準じる)

Scenario: 先頭サブタグのみを見る
  Given LANGUAGE=ja_JP:en_US:fr である
  When  プロンプトを組み立てる
  Then  日本語指示が含まれている (コロン区切りの先頭だけ評価する)

Scenario: 非jaのLANGUAGEは無意見である
  Given LANGUAGE=fr:en である
  When  プロンプトを組み立てる
  Then  従来出力とバイト同一である
```

## 受け入れ基準
- [x] `LANGUAGE` のコロン区切り先頭が `ja` 系サブタグのとき日本語になる
  (正規化は `locale_is_japanese` と同一: 小文字化・修飾除去・先頭照合)
- [x] 実効ロケールが `C` / `POSIX` のとき `LANGUAGE` は無視される
- [x] 全体の優先順位 (`--language` > config > ロケール > 既定) が崩れない
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green
- [x] `docs/specs/cli.md` の言語の段落が同一変更で更新されている

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `LANG=C.UTF-8 LANGUAGE=ja_JP:ja` での `optimize --print` 出力に日本語指示が含まれること

### 統合テスト
- 解決順の表テストに `LANGUAGE` 列を追加 (LC_ALL=C が LANGUAGE に勝つ境界つき)

### 単体テスト
- `LANGUAGE` 値の分解テスト (`ja_JP:en` → ja、`fr:en` → 非ja、空 → 非ja)
- C 優先の境界値テスト

## 実装アプローチ
- **Outside-In**: E2Eテストから開始し、失敗を確認してから実装
- **Red-Green-Refactor**: TDDサイクルを各レイヤーで適用
- **リファクタリング**: グリーンになるたびに品質改善

## 見積もり
2 (要チームでの見積もり。内訳目安: 解決拡張 1、テスト/spec 1)

## 技術的考慮事項
- 依存関係: `resolve_locale_language` の純粋性を保つこと。環境変数の読み取りは
  呼び出し側 (`process_locale_language` 相当) で行い、解決関数は引数で受け取る
  (PBI 2026-11-11-01 の流儀を踏襲)
- テスタビリティ: テストは合成値のみ。実環境の `LANGUAGE` に依存しない
- 非機能要件: 優先順位の決定は交渉可能。ただし `C` 優先だけは gettext 準拠で固定

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること）
```bash
# LANGUAGE 参照が無いことの再確認
grep -rn "LANGUAGE" src/ | head
# 解決関数の現行形
grep -n "resolve_locale_language\|process_locale_language\|locale_is_japanese" src/session.rs | head -n 20
```

### 実装手順
1. E2Eテストを Red で書く (`LANGUAGE=ja_JP:ja` での `--print` 出力)
2. 解決順の統合テストを Red で書く (C 優先の境界つき)
3. 分解ロジックの単体テストを Red で書く
4. ロケール段に `LANGUAGE` を足して Green にする
5. spec を同一変更で更新する

### 落とし穴
- `LANGUAGE` 全体をそのまま比較しない (コロン区切り・先頭のみ・正規化を忘れないこと)
- `std::env::var` を解決関数の内部で直接読まない (純粋性が崩れてテスト不能になる)
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] テストカバレッジが基準を満たす (E2E/統合/単体すべて)
- [x] コードレビュー完了
- [x] リファクタリング完了 (グリーン後)
- [x] ドキュメント更新済み
