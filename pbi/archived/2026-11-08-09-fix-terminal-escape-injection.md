# PBI: テーブル描画の terminal escape 注入を修正する

## ユーザーストーリー
saniwalens の表出力を読むユーザーとして、transcript 由来の文字列に
ANSI escape が混ざっていても、端末が誤作動せず表が崩れないようにしたい。
なぜなら現状はセルが逐語的に描画され、own-transcript 由来の ESC バイトが
そのまま端末に届くからだ。

## 優先度
- 順位: 09 / 全17件
- RICEスコア: 5.6（Reach=7 / Impact=0.5 / Confidence=80% / Effort=0.5日）
- 根拠: 全テーブル描画に影響する（Reach は大）が、self-data のみなので
  Impact は低い。修正は描画一点に集約できコストは小さい。

## BDD受け入れシナリオ
Scenario: Table 形式で escape が無害化される
  Given  セルに ANSI escape を含む文字列（例: agent 名や skill id に混ざった
  `\x1b[2J` や `\x1b]0;...`）がある
  When  ユーザーが Table 形式で表示する
  Then  escape は除去・無害化されて表示される
  And   列幅の計算がずれない

Scenario: Markdown 形式でも escape が無害化される
  Given  セルに ANSI escape を含む文字列がある
  When  ユーザーが Markdown 形式で表示する
  Then  escape は除去・無害化されて表示される
  And   Markdown の表構造（`|` 区切り）が壊れない

Scenario: 境界 — 正常文字列と日本語幅は変わらない
  Given  セルに escape を含まない通常文字列（ASCII・日本語混じり）がある
  When  ユーザーが Table / Markdown 形式で表示する
  Then  表示内容が従来どおりである
  And   既存ビューの出力が変化しない（capture の diff）

## 受け入れ基準
- [x] `render()` の Table・Markdown 両パスでセルから ANSI escape が除去される
- [x] 除去は幅計算の前に適用される（Table の列幅がずれない）
- [x] 正常文字列（escape 無し、ASCII・日本語混じり）の表示が変わらない
- [x] ヘッダーは固定文字列のため対象外でよい（セル側のみで可）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green（※ 自分の差分は clean。共有ツリー上の他者作業中ファイル起因の
  残警告・fmt 差分あり。詳細は実装レポート参照）

## テスト戦略
- 単体: sanitize helper の真偽表（CSI `ESC [` 系、OSC `ESC ]` 系、単発 ESC、
  素通しすべき通常文字列・日本語）。除去後に幅計算が正しいこと
- 統合: escape 入りセルを含む合成行で Table・Markdown 両パスの描画が
  無害化されること。正常行の既存 capture が変化しないこと
- E2E: 不要（描画層のみの変更）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状: src/cli/report.rs:81-119 の `render()` がセルを逐語的に描画する。
  Markdown パスは `row.join(" | ")`（89 行目）、Table パスは `format_row`
  （102-109 行目）と `println!`（115 行目）で、どちらのパスもサニタイズしない。
  cli/ に sanitize helper は存在しない
- ESC が届く経路（いずれも own-transcript 由来の非信頼文字列が `println!`
  に届く）: agent / skill の id（mod.rs:1989 の `row.agent`、
  mod.rs:2011 の `row.skill`）、kind / id（mod.rs:2197-2199 の
  `entry.kind` / `entry.id`）、project / file（mod.rs:1390-1391 の
  `e.project` / `e.file()`）、friction（mod.rs:1552-1559 の project・category、
  mod.rs:952-975 の doctor 表示の label・file）。`render()` 一点の修正で
  全経路を覆える
- 修正方向: `render()` 内で両形式のセルから ANSI escape を除去する
  （Table は幅計算の前に除去する。現状 96-101 行目の幅計算は
  `cell.chars().count()` のため escape が幅をずらす）。helper は
  cli/report.rs 側に置くか、依存追加なしの自前除去かは実装時に決める
  （依存を足す場合は license 許可一覧との整合を確認する）
- self-data のみが到達するため Impact は低いが、描画の単一 choke point で
  潰すことで将来のビュー追加にも効く
- エビデンス: src/cli/report.rs（81-119 行目の `render`、89 行目の Markdown
  join、102-115 行目の Table `format_row` と幅計算・出力）、src/cli/mod.rs
  （上記の各セル供給箇所）
- 関連コード: src/cli/report.rs:121-127 の `pad`（幅計算と埋め草の前提）、
  各ビューの capture テスト（正常出力の不変確認）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（CSI/OSC/単発ESC/正常文字列・
  Table幅・Markdown行の6テスト、`cargo test report` green）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [ ] ドキュメント更新済み（契約不変のため spec 更新不要と判断。レビュー時に確認）
