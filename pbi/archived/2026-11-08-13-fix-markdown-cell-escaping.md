# PBI: Markdown セルのエスケープを修正する

## ユーザーストーリー
`--format markdown` でレポートを貼り付ける saniwalens ユーザーとして、
skill id やパスに `|` や改行が含まれていても表が崩れないようにしたい。
なぜなら素の文字がそのまま列区切りや行区切りとして解釈されると、
列ずれ・行割れした壊れた Markdown が出力されてしまうからだ。

## 優先度
- 順位: 13 / 全17件
- RICEスコア: 2.4（Reach=3 / Impact=0.5 / Confidence=80% / Effort=0.5日）
- 根拠: 通常のデータ文字による --format markdown 出力破損を直す。
  影響は Markdown 分岐に限定され、aligns の sep（"---"）は変えない。

## BDD受け入れシナリオ
Scenario: `|` を含むセルは列を割らずに出力される
  Given skill id やパスに `|` を含む行がある
  When  ユーザーが --format markdown でビューを見る
  Then  その `|` はエスケープされ、行の列数がヘッダーと一致する

Scenario: 改行を含むセルは行を割らずに出力される
  Given セル値に改行を含む行がある
  When  ユーザーが --format markdown でビューを見る
  Then  改行はエスケープ（または同等の置換）され、1 論理行が
  And   1 物理行として出力される

Scenario: 通常のセルは従来通り出力される
  Given `|` も改行も含まない行がある
  When  ユーザーが --format markdown でビューを見る
  Then  出力は従来と同一である（ヘッダーと "---" の sep 行は不変）

## 受け入れ基準
- [x] Markdown 分岐のセル描画で `|` がエスケープされる
  （例: `\|`。列区切り " | " と衝突しないこと）
- [x] Markdown 分岐のセル描画で改行（`\n` / `\r\n`）がエスケープまたは
  置換され、行形状が崩れないこと（`<br>` に統一）
- [x] ヘッダー行と sep 行（"---"）の描画は変えない（aligns の sep は "---" のまま）
- [x] table / json 出力の描画は変化しないこと
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: `|` 含みセル（先頭・末尾・連続・複数列）、改行含みセル
  （`\n`、`\r\n`、末尾改行）、通常セルの無変換。ヘッダー側に特殊文字が
  入った場合の扱いも固定する
- E2E / capture: `|`・改行を含む合成データで --format markdown を実行し、
  各行の列数がヘッダーと一致すること。既存ビューの Markdown capture が
  通常データでは変化しないこと（diff）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 起点: src/cli/report.rs:85-89 の Markdown 分岐が生セルを " | " で
  join している。skill id / パスの素の `|` が列を割り、改行が行形状を壊す
- 修正方向: Markdown 分岐のセル描画側で `|` と改行をエスケープする。
  aligns の sep（"---"）は対象外で変えない
- エスケープ規約（`\|` か HTML 実体参照か、改行の置換先）はこの変更内で
  決めて単体テストで固定する。GFM 表として読める方を選ぶ
- 関連コード: src/cli/report.rs の render（Markdown 分岐のみ）
- spec 更新の要否は実装時に判断する（描画規約に触れる場合のみ）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec更新不要と判断: 描画規約はrustdocに記述。table/json不変）
