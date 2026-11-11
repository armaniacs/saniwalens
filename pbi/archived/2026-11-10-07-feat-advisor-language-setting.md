# PBI: advisor 出力の言語設定 (`language`)

## ユーザーストーリー
日本語で使いたい人として、advisor の説明・提案文を日本語で出したい。
なぜなら英語の長文プランを毎回読み解くのが負担だからだ。

## ビジネス価値
- 非英語話者の適用判断コストが下がる (y/n の判断が速くなる)
- 測定方法: `language = "ja"` のもとで advisor 指示文に日本語応答指示が含まれ、
  既定 (未設定) では従来出力とバイト同一であること

## BDD受け入れシナリオ

Scenario: 日本語設定で指示文に日本語応答指示が入る
  Given 設定ファイルに `language = "ja"` がある
  When  `optimize --print` でプロンプトを出す
  Then  advisor への指示に日本語で応答する旨が含まれている

Scenario: 未設定では何も変わらない
  Given 言語設定がない
  When  各種プロンプトを出す
  Then  従来出力とバイト同一である

Scenario: 不正値はエラーになる
  Given 設定ファイルに未知の言語値がある
  When  実行する
  Then  有効値の一覧つきで失敗する

## 受け入れ基準
- [x] 設定項目 `language` が config.toml のトップレベルにある (`default_harness` と同列)
- [x] `--language` グローバルフラグで上書きできる (フラグ > ファイル > 既定の解決順)
- [x] 有効値は `en` / `ja` から始める (拡張は別 PBI)。未知値は有効値つきエラー
- [x] 対象は advisor 向け指示文のみ (briefing 指示・ヘッドレス指示・apply 指示・起動ポインタの言語行)。
  レポートの表自体は英語のまま (非目標として明示)
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green
- [x] `docs/specs/cli.md` の config 節と optimize 節が同一変更で更新されている

## テスト戦略 (t_wadaスタイル)

### E2Eテスト
- `optimize --print` の出力に言語指示行が含まれること (実起動なしで検証可能)

### 統合テスト
- 解決順 (フラグ > ファイル > 既定) のテスト。`resolve` 周りの既存流儀に従う

### 単体テスト
- 指示文ビルダーの言語分岐 (ja で指示あり、未設定でバイト同一)
- 不正値エラーの文言テスト

## 見積もり
3 (要チームでの見積もり。内訳目安: 解決 1、指示文 1、テスト/spec 1)

## 技術的考慮事項
- 依存関係: 既存 config 解決 (`src/adapter/config_file.rs` の `resolve`) と
  プロンプト組み立て (`src/core/optimize.rs`)。`HarnessCopy` に足すか別経路かは実装者が選ぶ
- 言語指示は一文の定型とし、各指示文に散らばる書き方をしない (単一の文言ソース)
- レポート表の日本語化は本 PBI の範囲外。advisor 出力のみ

## 実装者向け注記

### 現状コードの確認
```bash
grep -n "default_harness\|resolve" src/adapter/config_file.rs | head -n 20
grep -n "instructions\|launch_prompt" src/core/optimize.rs | head -n 20
```

### 落とし穴
- 未設定時のバイト同一を壊さないこと (golden/既存テストが検出するはず。壊れたら即戻す)
- 日本語指示文自体の文字種は SHIFT-JIS 範囲に収めること
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] テストカバレッジが基準を満たす (E2E/統合/単体すべて)
- [x] コードレビュー完了
- [x] リファクタリング完了 (グリーン後)
- [x] ドキュメント更新済み
