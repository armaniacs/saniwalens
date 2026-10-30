# PBI: Kilocode の files_in_context[] を活用する

## ユーザーストーリー
Kilo Code を使う saniwalens ユーザーとして、task_metadata.json の files_in_context[] を読み、タスク単位の files-in-context を提示する機能がほしい。なぜなら Kilocode のコンテキスト実態（どのファイルが実際に文脈に載っていたか）が見え、未使用設定や摩擦の判断材料になるからだ。

## 優先度
- 順位: 23 / 全28件
- RICEスコア: 2.0（Reach=1 / Impact=2 / Confidence=2 / Effort=2日）
- 根拠: 単一ハーネス（kilocode）限定で Reach=1。データは task_metadata.json に既に存在するため小さく済む。依存関係はなし。

## BDD受け入れシナリオ

Scenario: files_in_context のパスがタスク単位で提示される
  Given files_in_context[] を持つ task_metadata.json が付いた Kilocode タスクがある
  When  ユーザーがそのタスクの解析結果を表示する
  Then  files_in_context のパスがタスク単位で提示される

Scenario: メタデータが無いタスクは通常の取り扱いのまま表示される
  Given task_metadata.json が無い（または files_in_context を持たない）Kilocode タスクがある
  When  ユーザーがそのタスクの解析結果を表示する
  Then  そのタスクはメタデータが無い場合の通常の取り扱いのまま表示される
  And   解析はエラーにならず続行する

## 受け入れ基準
- [ ] task_metadata.json の files_in_context[] を読み、タスク単位の files-in-context が提示される
- [ ] メタデータが無いタスク、files_in_context が空・欠損のタスクは通常の取り扱いのまま、エラーにならない
- [ ] docs/specs/session-format-kilocode.md 182 行付近（files_in_context[] は future material として v1 未読）を同じ変更内で更新する
- [ ] claude / opencode ハーネスの解析動作とストアは一切変わらない（kilocode 専用の変更）
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: --harness kilocode で合成タスク（メタデータあり / なし）を解析し、files-in-context の提示と通常取り扱いを確認する
- 統合: files_in_context のパスがタスクに紐付いて kilocode ストアに記録され、他ハーネスのストアに混入しないこと
- 単体: files_in_context の抽出の境界（空配列、キー欠損、型違い、パス表記のゆらぎ）、タスクとの対応づけ

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- spec 更新必須: docs/specs/session-format-kilocode.md 182 行付近「task_metadata.json (files_in_context[])」の行（never-fires rule の将来材料として未読扱い）と、28 行付近の tree comment「files_in_context[] only (unread in v1)」。読み取りを v1 以降の対象にする旨を書くこと。
- 実装の置き場所: 読み取りは src/adapter/kilocode.rs（adapter がハーネス入力形式の知識を独占する原則、.claude/rules/format-isolation.md）。ハーネス独立ストアの原則に従い、kilocode のデータは kilocode ストアのみに入る。
- 用途の想定: files_in_context[] は surfaces.md の never-fires rule（rule の paths: glob に一致するファイルが tool_use の対象として現れたかの証拠）の材料。この PBI では「タスク単位の提示（見える化）」までとし、never-fires 計算への接続は別候補にして scope を広げない。
- 判断材料: 同 spec の「What is deliberately not mapped (v1)」節。files_in_context[] は将来材料として位置づけられていたもの。提示形式は既存ビュー（sessions 系）の慣例に従う。
- privacy: 実タスクのメタデータはコミットしない。fixture は合成（.claude/rules/session-data-privacy.md）。
- 検証コマンド: cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test。

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
