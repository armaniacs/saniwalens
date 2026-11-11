# PBI: ロケール環境変数からの advisor 言語解決

## ユーザーストーリー
日本語ロケールの利用者として、`--language` も設定ファイルも触らずに
advisor を日本語で使いたい。なぜなら新しい変数名を覚えるより、
`LANG=ja_JP.UTF-8` という既存の環境がそのまま効く方が自然だからだ。

## ビジネス価値
- ゼロコンフィグで日本語になる (`LANG=ja_JP.UTF-8` の人は何もしない)
- 標準 Unix 作法 (独自変数を増やさない)
- 測定方法: フラグ・config 未指定で `LANG=ja_JP.UTF-8` のもと `--print`
  のプロンプトに日本語指示が含まれ、`LC_ALL=C` のもとでは含まれないこと

## 現状確認 (Phase 0 済み)
- `LANG` / `LC_ALL` / `LC_MESSAGES` / `locale` / `SANIWALENS_LANGUAGE` で
  `src/` を検索し、ロケール参照の実装は存在しないことを確認済み
- 言語系インターフェースは `--language` フラグと config の `language`
  のみで、環境変数由来の解決は存在しないことを確認済み
  (`SessionEnv` が読むのは state_home / home / config_home のみ)

## BDD受け入れシナリオ

```gherkin
Scenario: ロケールだけで日本語になる
  Given フラグにもconfigにも言語指定がなく、LANG=ja_JP.UTF-8 である
  When  プロンプトを組み立てる
  Then  日本語応答の指示が含まれている

Scenario: LC_ALL が LANG に勝つ
  Given LANG=ja_JP.UTF-8 かつ LC_ALL=C である
  When  プロンプトを組み立てる
  Then  日本語指示は含まれない (POSIX の優先順位どおり)

Scenario: 明示指定はロケールに勝つ
  Given config に language="ja" があり、LANG=C である
  When  プロンプトを組み立てる
  Then  日本語指示が含まれている
  And   --language en と LANG=ja_JP.UTF-8 では英語のままである

Scenario: 変な値は既定に落ちる
  Given LANG が未設定・空・"C"・"en_US.UTF-8" のいずれかである
  When  プロンプトを組み立てる
  Then  従来出力とバイト同一である
```

## 受け入れ基準
- [ ] 解決順が `LC_ALL` > `LC_MESSAGES` > `LANG` である (POSIX 準拠)
- [ ] 言語判定は先頭サブタグのみを見る (小文字化し、エンコーディング
  `.UTF-8` や `@` 修飾を除いた先頭が `ja` のとき日本語。`ja` /
  `ja_JP` / `ja_JP.UTF-8` が日本語、`en_US` / `C` / 空が英語)
- [ ] 全体の優先順位が `--language` フラグ > config > ロケール > 既定 (En) である
- [ ] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green
- [ ] `docs/specs/cli.md` の config 節と言語の段落が同一変更で更新されている

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `LANG=ja_JP.UTF-8` での `optimize --print` 出力に日本語指示が含まれること (実起動なし)

### 統合テスト
- 解決順のテスト (フラグ > config > ロケール > 既定の表テスト)

### 単体テスト
- 値の正規化テスト (`ja_JP.UTF-8` → ja、`JA-jp` → ja、`C` → 非ja、空 → 非ja)
- 優先順位の境界値テスト (LC_ALL=C + LANG=ja → 英語、config ja + LANG=C → 日本語)

## 実装アプローチ
- **Outside-In**: E2Eテストから開始し、失敗を確認してから実装
- **Red-Green-Refactor**: TDDサイクルを各レイヤーで適用
- **リファクタリング**: グリーンになるたびに品質改善

## 見積もり
2 (要チームでの見積もり。内訳目安: 解決 1、テスト/spec 1)

## 技術的考慮事項
- 依存関係: 言語解決 (`resolve_cli_with_language` 相当) と `SessionEnv`
  の流儀に従うこと。ロケール変数は呼び出し側で読んで渡し、
  解決自体は純粋に保つ (既存の XDG 解決と同じ形)
- テスタビリティ: 環境変数の読み取りを注入可能にし、テストは合成値のみ使う
- 非機能要件: 表本体は英語のまま (PBI 07 の範囲)。本 PBI は advisor 指示文のみ

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること）
```bash
# ロケール参照が無いことの再確認
grep -rn "LC_ALL\|LC_MESSAGES\|getenv(\"LANG" src/ | head
# 言語解決の現行形
grep -n "resolve_cli_with_language\|AdvisorLanguage" src/session.rs | head -n 20
```

未実装であることは Phase 0 で確認済み。上記で何か見つかれば調査してから実装に進むこと。

### 実装手順
1. E2Eテストを Red で書く (`LANG=ja_JP.UTF-8` での `--print` 出力に日本語指示)
2. 解決順の統合テストを Red で書く (フラグ > config > ロケール > 既定)
3. 値の正規化の単体テストを Red で書く (境界値つき)
4. 解決にロケール段を足して Green にする (呼び出し側で読む→渡す形を守る)
5. spec を同一変更で更新する

### 落とし穴
- `std::env::var` を解決関数の内部で直接読まない (純粋性が崩れてテスト不能になる。
  `env_roots` のように呼び出し側で読むこと)
- `ja_JP.UTF-8` のような値をそのまま比較しない (正規化してから先頭サブタグを見ること)
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] テストカバレッジが基準を満たす (E2E/統合/単体すべて)
- [ ] コードレビュー完了
- [ ] リファクタリング完了 (グリーン後)
- [ ] ドキュメント更新済み
