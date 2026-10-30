# PBI: hook / claude_md の usage 測定を拡張する

## ユーザーストーリー
hook や CLAUDE.md を使う saniwalens ユーザーとして、hook 発火を transcript から拾い、hook を usage-measurable に拡張する（claude_md は observed context の残差からの推定）機能がほしい。なぜなら Unused / Costly+rare の最適化ウェッジが hook と claude_md にも適用でき、設定全体のどこを削るべきかの網羅が進むからだ。

## 優先度
- 順位: 21 / 全28件
- RICEスコア: 2.0（Reach=2 / Impact=2 / Confidence=2 / Effort=4日）
- 根拠: usage-measurable kind の契約変更を伴う拡張のため、同スコアの中では後に置く。依存関係: surfaces.md の usage-measurable 契約の変更をこの PBI 内で実施する（他 PBI への先行依存はない）。

## BDD受け入れシナリオ

Scenario: 発火のあった hook は unused に載らない
  Given transcript に発火の記録がある hook が設定されている
  When  ユーザーが未使用 surface の候補を表示する
  Then  その hook は unused の候補に載らない

Scenario: claude_md の推定値は「推定」と明示されて表示される
  Given CLAUDE.md が設定されている
  When  ユーザーが claude_md の usage とコストを表示する
  Then  残差から推定した値には「推定」であることが明示される
  And   推定値が実測のように見える表示にはならない

Scenario: 発火の証拠が読み取れない hook は unused に誤載しない
  Given hook の発火が transcript 上で構造的に識別できない session がある
  When  解析を実行する
  Then  その hook は証拠が無いことを理由に unused として断定されない
  And   解析はエラーにならず続行する

## 受け入れ基準
- [ ] hook 発火が transcript から拾われ、hook が usage-measurable kind に加わる
- [ ] 発火のあった hook は unused（および Costly+rare）の候補に載らない
- [ ] claude_md の推定値は「推定」と明示されて表示される
- [ ] src/core/surface.rs:238 付近の「hook は非計測」と断言するテストを、新しい契約に合わせて更新する
- [ ] docs/specs/surfaces.md の usage-measurable kind 契約（Requires 列の適用範囲）を同じ変更内で更新する
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: hook 発火を含む合成 transcript と hook 設定で、unused 候補にその hook が載らないこと。claude_md の推定値に「推定」の表示があること
- 統合: hook 発火が usage イベントとして store に記録され、surface カタログとの結合で発火回数が付くこと。claude_md の残差推定が observed context から計算されること
- 単体: hook 発火の抽出（発火 / 非発火 / 識別不能の判定）、残差推定の計算と「推定」ラベル、更新後の usage-measurable 判定の真偽表

## 見積もり
4日（RICE の Effort と同一根拠）

## 実装者向け注記
- 更新必須のテスト: src/core/surface.rs:238 の rules_and_claude_md_are_catalog_only が hook と claude_md について非計測を断言している。hook を usage-measurable にするとこの断言が変わるため、契約変更とセットで更新する。claude_md を推定ベースで measurable にするかは着手時に決めてよいが、決定を注記に残すこと。
- spec-sync 必須: docs/specs/surfaces.md の Requires 列の節（130 行付近）「Unused and Costly + rare apply only to usage-measurable kinds」。usage-measurable kind の拡張はこの契約変更を伴う。
- claude_md の推定は architecture.md「always-on reconciliation」の延長（surfaces.md の「always-on reconciliation」節も参照）。近似は隠さずラベルするのが原則（architecture.md「What this tool deliberately is not」：Approximations are acceptable and flagged, not avoided）。
- adapter: hook 発火の抽出は transcript 側（src/adapter/transcript.rs。PreToolUse hook 由来の拒否テキスト分類が 412 行付近にあり、手掛かりになる）。transcript に hook 発火の構造的マーカーが無い場合は本文テキストフォールバックになり、その限界を spec に記すこと。hook の定義読み取りが config 側（settings.json の hooks）に存在するかを着手時に確認する。
- core は純粋のまま: usage-measurable 判定、wedge 分類、抽出済みレコードからの usage 集計は I/O フリーでユニットテストする（.claude/rules/tdd.md）。
- privacy: fixture は合成。実 transcript と実 config はコミットしない。
- 検証コマンド: cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test。

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
