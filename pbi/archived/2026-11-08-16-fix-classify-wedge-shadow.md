# PBI: classify_wedge のシャドウ分類を明示的に固定する

## ユーザーストーリー
saniwalens の保守担当者として、`classify_wedge` の wedge 優先順位が
将来の surface 種別追加でも黙って変わらないよう、シャドウしている分岐を
明示的に固定してほしい。なぜなら (measurable, StartupFull, heavy, uses==0)
の入力が現在は必ず AlwaysOnHeavy になり UNUSED に到達しないことが
コードリーディングでしか分からず、将来種別が増えたときに誤分類の
温床になるからだ。

## 優先度
- 順位: 16 / 全17件
- RICEスコア: 1.0（Reach=1 / Impact=0.5 / Confidence=50% / Effort=0.25日）
- 根拠: 潜在的なシャドウ（latent shadow）であり、現実には到達不能な分岐を
  安価にピン留めするだけ。動作変更ではなく分類規約の明示化が目的のため
  最下位群。

## BDD受け入れシナリオ
Scenario: measurable + StartupFull + heavy + uses==0 は文書化された優先順位に従う
  Given measurable=true、load_mode=StartupFull、static_tokens が heavy 閾値以上、
  uses==0 の surface がある
  When  `classify_wedge` で分類する
  Then  選択された wedge が仕様として明示される
  And   どちらの wedge が優先されるかの理由（Wedge::priority の順序意図）が
  テスト名またはコメントで読み取れる

Scenario: 既存の到達可能な分類は一切変わらない
  Given 既存の adapter が実際に作る組み合わせ
  （例: StartupDescription の skill で uses==0、StartupFull の catalog-only で heavy）
  When  `classify_wedge` で分類する
  Then  UNUSED / ALWAYS-ON HEAVY / COSTLY+RARE / なし の従来判定が変わらない

Scenario: uses==0 の measurable + 非 always-on は UNUSED のままである
  Given measurable=true、load_mode=StartupDescription、heavy 閾値未満でも以上でも、
  uses==0 の surface がある
  When  `classify_wedge` で分類する
  Then  Wedge::Unused が返る
  （優先順位の明示化が通常の UNUSED 判定を壊していないことの境界確認）

## 受け入れ基準
- [x] (measurable, StartupFull, heavy, uses==0) の分類結果が単体テストで
  ピン留めされ、期待値がコード上で明示される
- [x] ピン留めが「意図した優先順位」であることが読み取れる
  （Wedge::priority との対応付け、または reorder した場合はその理由の文書化）
- [x] 既存の分類テスト（未使用 / catalog-only / always-on heavy / costly-rare /
  well-used）が全て green のままである
- [x] docs/specs/surfaces.md の wedge 分類の記述と矛盾しない
  （必要なら優先順位の一文を追記。不要なら追記なしの判断を残す）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: `classify_wedge(true, StartupFull, heavy, 0, threshold)` のピン留めテストを
  追加。heavy 境界値（閾値ちょうど / 閾値-1）、uses 境界（0 / 1 / 3）での
  分類表テスト。既存テスト群の回帰実行
- 統合: 不要（pure core の分類関数のみ。adapter や store の変更なし）
- E2E: 不要（waste ビュー等の出力変化なしのはず。変えた場合は capture の diff で
  無変化を確認する）

## 見積もり
0.25日（RICE の Effort と同一根拠。ピン留めテスト + コメント/文書化のみ）

## 実装者向け注記
- 対象: src/core/surface.rs の `classify_wedge`（210-214行目）。現在の順序は
  先に `load_mode.is_always_on() && heavy` で AlwaysOnHeavy を返し（210-212）、
  次に `measurable && uses == 0` で Unused を返す（213-215）。そのため
  (measurable, StartupFull, heavy, uses==0) は前者で確定し後者に到達しない
- 現在到達不能な理由（直さなくてよいが注記に残す）:
  (1) skills / agents は StartupDescription であり（LoadMode 定義 36-47、
  `is_always_on` は StartupFull のみ true: 62-64）、
  (2) StartupFull を取る種別（rule / hook / CLAUDE.md 等）は `is_usage_measurable`
  （111-115: skill | agent | mcp_server | mcp_tool | permission のみ true）に
  含まれないため measurable=true と StartupFull が交差しない
- 優先順位は意図的である: `Wedge::priority`（147-153）は AlwaysOnHeavy=0、
  Unused=1、CostlyRare=2 の順であり、分類の if 順と一致する。なので単純な
  バグではなく「暗黙の優先順位がテストで固定されていない」問題として扱う
- 修正方向（どちらかを選んで PBI 内で一貫させる）:
  (a) 順序を変えずピン留めテスト + コメントで「AlwaysOnHeavy 優先が意図的」と
  明示する（最小差分。推奨）、
  (b) Unused チェックを先にし、Wedge::priority の順序と docs/specs/surfaces.md の
  wedge 記述を合わせて更新する（分類の意味論変更になるため waste ビューの
  capture 回帰が必須）。(b) を選ぶ場合は理由をこの PBI の受け入れ基準に追記する
- 関連コード: src/core/surface.rs（HEAVY_TOKENS: 108、classify_wedge: 202-220、
  既存テスト: 243-277）、src/core/findings.rs（wedge ルーティング）、
  src/cli/mod.rs（waste ビュー）
- エビデンス: docs/specs/surfaces.md（wedge 表と catalog-only / measurable の節）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了
- [x] ドキュメント更新済み（必要な場合のみ。不要判断も可だが理由を残す）
