# PBI: parse_timeout の巨大入力 panic を修正する

## ユーザーストーリー
`--timeout` を指定する saniwalens ユーザーとして、巨大な値を渡しても
CLI が panic で落ちずに正規のエラーになるようにしたい。なぜなら
panic はクラッシュベクタであり、引数エラーはエラーメッセージで
返るべきだからだ。

## 優先度
- 順位: 14 / 全17件
- RICEスコア: 2.0（Reach=1 / Impact=1 / Confidence=50% / Effort=0.25日）
- 根拠: --timeout 引数経由の CLI クラッシュベクタを潰す。
  到達は稀だが panic は確実に避けるべき。

## BDD受け入れシナリオ
Scenario: 通常の --timeout 値は従来通り Duration になる
  Given `--timeout 30s`、`2m`、`500ms` などの通常値
  When  ユーザーがその値でコマンドを実行する
  Then  対応する Duration として受理される

Scenario: 巨大だが有限の値は panic せずエラーになる
  Given `Duration::from_secs_f64` が panic する巨大だが有限の入力
  （例: 1e308s）
  When  ユーザーがその値でコマンドを実行する
  Then  プロセスは panic せず、正規のエラーで終了する

Scenario: 非有限・ゼロ・負値は従来通り拒否される
  Given `inf` / 非数値 / ゼロ / 負の --timeout 値
  When  ユーザーがその値でコマンドを実行する
  Then  従来通りエラーメッセージで拒否される（panic しない）

## 受け入れ基準
- [x] 有限だが `Duration::from_secs_f64` の範囲を超える入力が
  panic ではなく `Result::Err`（正規の anyhow エラー）になる
- [x] 既存の拒否（inf / 非有限、ゼロ、負、不正書式）は維持される
- [x] 通常値（`30s`、`2m`、`500ms`、素数値＝秒）の受理は変化しない
- [x] 上限（range-cap）の値とエラーメッセージ文面を決めてテストで固定する
  （`try_from_secs_f64` の範囲＝u64::MAX秒。文面 `--timeout too large`）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: 境界値（上限ちょうど / 上限超過 / 1e308s / 単位別 `ms`・`s`・`m` での
  オーバーフロー）、既存拒否（inf、NaN、不正書式、ゼロ、負）、通常値の受理。
  panic しないことは `#[should_panic]` ではなく Err の assertion で固定する
- E2E（任意）: CLI 経由で巨大値を渡し、非ゼロ終了＋エラーメッセージに
  なること（スタックトレース／panic 出力が出ないこと）

## 見積もり
0.25日（RICE の Effort と同一根拠）

## 実装者向け注記
- 起点: src/cli/headless.rs:198-213 の parse_timeout が
  `Duration::from_secs_f64(amount * unit_secs)` を無検査で呼ぶ。
  :210 で inf / 非有限は既に拒否されているが、巨大だが有限の入力
  （例: 1e308s）は素通りして panic する
- 修正方向: 有限入力に range-cap を設け、範囲外は正規のエラーで返す。
  `Duration::try_from_secs_f64`（または事前の上限比較＋ saturating ではなく
  エラー化）のいずれかで panic 経路を消す
- 単位乗算（`amount * unit_secs`）自体のオーバーフロー（f64 で inf 化）も
  考慮し、乗算後の非有限もエラー側に倒す
- 関連コード: src/cli/headless.rs の parse_timeout のみ
- spec 更新の要否は実装時に判断する（CLI の --timeout 契約に触れる場合のみ）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec更新不要と判断: rustdocに契約記述。CLIの--timeout語彙不変）
