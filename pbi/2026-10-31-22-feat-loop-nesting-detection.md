# PBI: loop（meta-skill）ネスト検出を design spike から実装する

## ユーザーストーリー
saniwalens ユーザーとして、transcript から meta-skill（loop）の親子判定を検出し span に反映する機能がほしい。なぜならネストした session のコスト帰属が正確になり、loop が呼び出した子 skill のコストが誤って別の並列コストとして見える誤読がなくなるからだ。

## 優先度
- 順位: 22 / 全28件
- RICEスコア: 2.0（Reach=2 / Impact=2 / Confidence=2 / Effort=4日）
- 根拠: 設計判断が未決のため、前半に design spike を含め、確度が上がるまで実装に進まない。依存関係はなし（既存 span 抽出の延長）。

## BDD受け入れシナリオ

Scenario: spike で決めた方式で親子関係が検出され span に反映される
  Given meta-skill（loop）が他の skill を呼び出した transcript を用意する
  When  解析を実行する
  Then  spike で決めた方式で親子関係が検出され、span に反映される
  And   子 skill のコストが loop の span に帰属することが分かる

Scenario: 判定できない session は現行の flat-span の扱いのまま落ちない
  Given 親子判定に必要な手掛かりが無い transcript を用意する
  When  解析を実行する
  Then  その session は現行の flat-span の扱いのまま解析が続く
  And   判定できないことをエラーや警告の嵐にしない

## 受け入れ基準
- [ ] 前半に design spike を置く：候補方式の比較、判断材料、決定と根拠を docs/specs に記録する
- [ ] spike で決めた方式で meta-skill（loop）の親子関係が検出され span に反映される
- [ ] 判定できない session は現行の flat-span の扱いのまま落ちない（既存の span 出力を壊さない）
- [ ] docs/specs/events.md「Meta-skills and nesting」節（105 行付近）を新しい契約に更新する（spec 更新必須）
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: loop が子 skill を呼び出した合成 transcript で、親子判定が span に反映されること。手掛かりの無い transcript で flat-span のまま解析が続くこと
- 統合: 親子判定済み span が store に記録され、コスト帰属（親へのロールアップ、子の区別）が正しいこと
- 単体: 親子判定の決定表（判定できる条件 / できない条件の網羅）、境界（同一時刻の呼び出し、連続呼び出し、自己呼び出し、子が複数）

## 見積もり
4日（RICE の Effort と同一根拠：design spike を含む）

## 実装者向け注記
- 重要: 設計判断が未決。README.md 224 行付近「Not yet implemented: meta-skill (loop) nesting」（親子判定の設計決定が pending）、src/core/span.rs 161 行付近「Nesting and subagent attribution are not yet applied (see docs/specs/events.md)」。この PBI は前半に design spike（方式比較、docs/specs 更新、決定の記録）を置き、後半に実装を置くこと。
- 現行 spec の立場: docs/specs/events.md「Meta-skills and nesting」節（105 から 117 行付近）。構造的マーカー（source、promptId、nesting depth）が無く、時間窓の推定や meta-skill のハードコード一覧は脆弱と判断して意図的に flat にしている。spike はこの判断を再検討し、誤帰属のリスクと保守性で候補を比較して決める。取り返しが付きにくい決定（meta-skill 名のハードコード等）は慎重に。
- 実装の置き場所: span 抽出は純粋 core。src/core/span.rs の extract_spans（161 行付近）は records からの純粋関数なので、親子判定も records を入力とする純粋な決定として入れる。subagent 帰属（src/core/subagent.rs）との整合を確認すること。
- spec 更新必須: docs/specs/events.md（採用方式と決定の記録）。README の「Not yet implemented」行も同じ変更内で更新する。
- TDD: 赤、緑、リファクタ。既存 span のテストを壊さない。後方互換（判定できない入力で flat-span のまま）のテストを最初に書くのが安全。
- privacy: fixture は合成。
- 検証コマンド: cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test。

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
