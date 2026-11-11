# PBI: ガイドから適用ハンドオフへの接続

## ユーザーストーリー
ガイドを使う人として、「次の一手を実行」でヘッドレス harness の場合も保存→対話適用まで進みたい。なぜなら今は通常 optimize が走るだけで、適用には別途 `--apply` が要り、二度手間だからだ。

## ビジネス価値
- ガイド内で doctor から適用まで一筆書きになる (opencode 利用者の動線が完成する)
- 測定方法: ガイドの実行後にプランファイルが残り対話適用が起動すること

## BDD受け入れシナリオ

Scenario: ヘッドレス harness ではハンドオフで実行される
  Given ガイドで opencode を選びレポートまで進んでいる
  When  「次の一手を実行」に y と答える
  Then  通常 optimize ではなくハンドオフ (`--apply` 相当) が走る
  And   プランファイルのパスが表示され対話適用が起動する

Scenario: 対話 harness では従来どおり
  Given ガイドで claude を選んでいる
  When  「次の一手を実行」に y と答える
  Then  従来どおり対話 optimize が起動する (ハンドオフは挟まらない)

Scenario: いいえと答えれば何も起きない
  Given 「次の一手を実行」の問いがある
  When  n / 空 / q / EOF で答える
  Then  何も起動せず保存の問いへ進む

## 受け入れ基準
- [x] 実行方式の分岐が harness の実行形態で決まる (ヘッドレス→ハンドオフ、対話→従来)
- [x] 分岐の理由が spec に一行で書かれている (対話に渡す相手がいない/いる)
- [x] E2E スクリプトが新しい問いに対応している (全スクリプトの入力位置を確認すること)
- [x] 失敗時 (対話起動失敗など) はガイドが死なず保存の問いへ進む (既存の流儀どおり)
- [x] `cargo test` と `cargo clippy --all-targets -- -D warnings` が green
- [x] `docs/specs/cli.md` のガイド節が同一変更で更新されている

## テスト戦略 (t_wadaスタイル)

### E2Eテスト
- n 回答の全フロー (何も起動しないこと)。y 回答の実起動は対象外としコメントで明示する

### 統合テスト
- 分岐条件のテスト (harness ごとの実行方式選択)

### 単体テスト
- 問いと分岐のテスト (Cursor 駆動)

## 見積もり
3 (要チームでの見積もり。内訳目安: 分岐 1、E2E 更新 1、テスト/spec 1)

## 技術的考慮事項
- 依存関係: `--apply` ハンドオフとガイド内実行 (`feature/optimize-apply-handoff` 作業ツリー) が前提
- 分岐は起動方式のデータ (`RunMode` 相当) で決め、harness 名の直書き分岐を増やさないこと
- E2E スクリプトは問いが1つ増える可能性に注意し、全スクリプトの入力位置を数え直すこと

## 実装者向け注記

### 現状コードの確認
```bash
grep -n "次の一手を実行" src/cli/views/guide.rs | head
grep -n "run_apply\|run_headless\|RunMode" src/cli/handoff.rs src/cli/headless.rs src/adapter/harness.rs | head -n 20
```

### 落とし穴
- ガイドの StdinLock を握ったまま対話に引き渡す注意書きは既存のまま。入力方式は変えない
- 実 transcripts / 実 config をテストに使わない。fixture は合成のみ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする (E2E 除外はコメントで明示)
- [x] テストカバレッジが基準を満たす
- [x] コードレビュー完了
- [x] リファクタリング完了 (グリーン後)
- [x] ドキュメント更新済み
