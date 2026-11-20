# PBI: guide の TOML 行編集 2 関数の骨格を統合する

## 優先度
- 順位: 10 / 全14件
- RICEスコア: 3.0（Reach=3 / Impact=1 / Confidence=100% / Effort=1日）
- 根拠: render_default_harness と render_profile が「既存 TOML を行配列として読み、
  セクション追跡しつつ `key = ` 行を置換、無ければ挿入位置を決めて追記」という
  同一の編集骨格をそれぞれ約 40 行で実装している。行単位 TOML 編集はコメント保存・
  テーブル境界判定など壊れやすい契約を含み、2 実装がドリフトすると「ガイド経由の
  config 書き込みだけが壊れる」事故になりやすい。共通ヘッダ文字列の const も
  すでに 2 箇所に複製済み

## ユーザーストーリー
メンテナとして、ガイドの config 書き込み（default_harness 保存 / profile 保存）の
行単位 TOML 編集が 1 つの実装に集約された状態がほしい。なぜなら同じ壊れやすい
契約（コメントの逐語保存・テーブル境界判定）を 2 箇所で別々に守っていると、
片側だけの修正漏れや片側だけの破壊が既存テストでも検出されにくく、
ドリフトが静かに蓄積するからだ。

## 背景
- `src/cli/views/guide.rs:252-289` — render_default_harness:
  first `[table]` 前挿入型の行単位 upsert。`default_harness = ` 行を
  テーブル外で置換し、無ければ最初の `[` テーブル行の直前に挿入する
- `src/cli/views/guide.rs:304-351` — render_profile:
  セクション末挿入型。対象 `[profiles.<name>]` セクション内の `harness = ` 行を
  置換し、無ければ次のテーブルヘッダ直前またはファイル末尾（セクション末）に
  挿入する。セクション自体が無ければ新規テーブルとして追記する
- `src/cli/views/guide.rs:253` と `src/cli/views/guide.rs:305` —
  同一内容の HEADER const（`# saniwalens defaults (written by the guided TUI).`）
  が二重定義。テーブルヘッダ判定の table_header ヘルパー
  （`src/cli/views/guide.rs:291-300`）は render_profile 専用に置かれている
- 改善案: 「行配列の走査＋セクション追跡」を共通化し、対象キー一致述語と
  挿入戦略（先頭テーブル前／セクション末）をクロージャか enum パラメータで
  受ける 1 つの `upsert_toml_line` に統合する。既存の行単位ゴールデン相当テスト
  （guide.rs 内 tests）はそのまま流用でき、書き込み結果はバイト同一
  （既存 config への再実行で diff 空）である

## BDD受け入れシナリオ

```gherkin
Scenario: 既存 config への default_harness 再実行はバイト同一になる
  Given default_harness 行を含む既存の config.toml がある
  When  ガイドの保存で default_harness を同じ値で上書きする
  Then  出力は入力とバイト同一であり diff が空である

Scenario: 既存 config への profile 再実行はバイト同一になる
  Given [profiles.<name>] に harness 行を含む既存の config.toml がある
  When  ガイドの保存で同じ profile に同じ harness を上書きする
  Then  出力は入力とバイト同一であり diff が空である

Scenario: コメントと他セクションはどちらの保存経路でも逐語保存される
  Given コメント・空行・[profiles.<他>] を含む config.toml がある
  When  ガイドの保存（default_harness / profile のいずれか）を行う
  Then  コメント・空行・他セクションは逐語保存される
  And   対象キーの行だけが置換または挿入される
```

## 受け入れ基準
- [x] 「行配列の走査＋セクション追跡」が 1 つの upsert_toml_line（仮称）に統合され、
  render_default_harness と render_profile から同一骨格が消えている
- [x] 対象キー一致述語と挿入戦略（先頭テーブル前／セクション末）はクロージャか
  enum パラメータで受け、呼び出し側がどちらの戦略かを明示する
- [x] 両関数の出力は統合前とバイト同一（既存 config への再実行で diff 空、
  新規ファイル生成のケースを含む）
- [x] HEADER const の二重定義が 1 箇所に統合されている
- [x] 既存の行単位ゴールデン相当テスト（guide.rs 内 tests）は無変更のまま green
- [x] 統合で既存テストがカバーしない分岐が残った場合にのみ、その分岐の
  ピン留めテストを足す（テストの乱造をしない）
- [x] 変更は src/cli/views/guide.rs に閉じている（同日の他 PBI は guide.rs に
  触れないため、衝突を起こさない）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: guide.rs 内 tests の行単位ゴールデン相当テスト
  （default_harness_save_* / profile_save_* / ask_save_*）が等価性の証人。
  無変更のまま green であることを完了条件とする
- 追加テスト: 基本不要。統合により既存テストが通らなくなる、または
  証人が不足している分岐（挿入戦略の分岐など）が判明した場合にのみ、
  最小のピン留めを足す
- E2E: tests/guide_flow.rs（スクリプト駆動のガイド E2E）が既存のまま green で、
  保存ダイアログ経由の config 書き込みが壊れていないことを担保する

## 見積もり
1日（RICE の Effort と同一根拠）。変更が guide.rs 内で完結し、
等価性の証人となる既存テストが揃っているため小さく見積もる

## Definition of Done
- [x] 全BDDシナリオが自動テストで実証される（既存ゴールデンの流用＋
  必要最小限の追加ピン留め）
- [x] render_default_harness / render_profile の出力が統合前とバイト同一である
- [x] 変更が src/cli/views/guide.rs に閉じている
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・
  spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（guide の config 書き込み契約に関する spec 記述が
  あれば同期し、該当がなければ更新不要と判断した）
