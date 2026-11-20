# PBI: store に住む共有 row 型を core へ移す

## 優先度
- 順位: 05 / 全14件
- RICEスコア: 10.0（Reach=5 / Impact=1 / Confidence=100% / Effort=0.5日）
- 根拠: 2026-11-09 の C7（findings 組立の core 移管、b843a70）と C8（契約 newtype
  導入、af7e250）の着地で、FrictionCell / ToolBreakdown / ProjectSlug / SessionRoot
  の 4 つのドメイン row 型が src/store.rs に定義された。結果、純粋コアの
  core/findings.rs が crate::store を import する方向逆転が発生した
  （b843a70 時点では store-free だったのが af7e250 で逆転）。
  docs/specs/architecture.md の一方向フロー（adapter → core → store → report、
  core は下位を import しない）がモジュールレベルで崩れている。
  トリップワイヤ（tests/pure_join_placement.rs）は join の所在のみ監視し、
  共有型の所在は未監視のため、この逆転は検知されないまま固定化される。
- 実行順の制約: store.rs に触れるのは 01（先行）→ 05（本件）→ 08（後続）の直列。
  同じバッチ2（05 / 06 / 07）で store.rs に触れるのは本件のみであり、
  06 / 07 は store.rs に触れてはならない

## ユーザーストーリー
メンテナとして、ドメインの契約 row 型が純粋コア側に住み、store はそれを
import するだけの一方向依存に戻ってほしい。なぜなら純粋コアが store の
名前空間に到達し始めると、core を単体で参照・再利用できなくなり、
architecture.md の層分離が「コメント上の規約」になっていくからだ。

## 背景
- C8 で導入された 4 型の定義は src/store.rs:358-404 に固まっている
  （FrictionCell :363 / ToolBreakdown :372 / ProjectSlug :382、impl :384 と :390 /
  SessionRoot :401）。
  同ファイルはこの型を返すクエリメソッド群を持つ
  （error_counts_by_project :1221 / error_tool_breakdown :1292 /
  error_tool_breakdown_briefing :1936 / error_counts_by_project_briefing :1972 /
  session_roots_briefing :2185）。
- 逆依存の実体は src/core/findings.rs:19 の
  `use crate::store::{FrictionCell, SessionRoot, ToolBreakdown};` と、
  同ファイルのテスト（src/core/findings.rs:685）の
  `crate::store::ProjectSlug("alpha".to_string())` 参照。
  src/core 配下で crate::store に到達しているのはこの 2 箇所のみ。
- 正方向の依存は既にある: src/store.rs:14-20 は crate::core 配下
  （period / span / subagent / surface / thrash / trend / usage）から型を
  import しており、store → core が層の許す唯一の方向。
- 監視の穴: tests/pure_join_placement.rs は 1st pattern（trend の two-window
  joins）の定義所在と src/core/trend.rs の crate::store 不使用のみ assert し
  （tests/pure_join_placement.rs:151-166、:199-208）、共有 row 型の所在は
  監視していない。findings.rs の逆転はどのテストも赤にしない。
- 改善案: 4 型の定義を core へ移す（core/findings.rs に置くか、row 型用の
  小さな core モジュールを切る）。src/store.rs 側は pub use で再エクスポートし、
  公開 API（crate::store::{FrictionCell, ...} パス）を維持する。
  本 PBI が store.rs に触れるバッチ2 唯一の PBI であり、01 が完了した後、
  08 より前に実施すること。

## BDD受け入れシナリオ

```gherkin
Scenario: 依存方向が一方向へ戻る
  Given 4 つの row 型の定義が core 配下へ移っている
  When src/core 配下を grep する
  Then findings.rs を含め crate::store を import・参照する箇所が 1 件も無い

Scenario: 既存の公開パスは壊れない
  Given store.rs が移動済みの型を pub use で再エクスポートしている
  When cargo build と cargo test を実行する
  Then crate::store::{FrictionCell, ToolBreakdown, ProjectSlug, SessionRoot}
       を参照する既存コードが全部そのままコンパイルされ、全テストが green になる

Scenario: 移動は振る舞いを変えない
  Given 移動前と移動後の同一入力（合成 fixture）
  When cargo test を実行してテスト一覧を比較する
  Then 実行されるテスト名の集合に差分が無い（型定義位置の移動のみで、
       テストの追加・削除・改名が無い）
```

## 受け入れ基準
- [x] FrictionCell / ToolBreakdown / ProjectSlug / SessionRoot の 4 型の定義が
  core 配下へ移っている（core/findings.rs、または row 型専用の小さな core
  モジュール。ProjectSlug の impl 2 件を含む）
- [x] src/store.rs は移動済みの 4 型を pub use で再エクスポートし、
  crate::store 側の既存参照（:1221 / :1292 / :1936 / :1972 / :2185 の
  クエリメソッド群を含む）が無修正のままで動く
- [x] src/core/findings.rs:19 と :685 の crate::store 参照が crate::core 側の
  パスへ向き、src/core 配下の crate::store 出現が 0 件になる
- [x] 移動で変わったのが型定義位置のみであることを diff で確認する
  （テスト名の diff が空。テスト本体の修正は import 経路の更新のみ）
- [x] store.rs への変更は本 PBI の移動・再エクスポートに留める
  （01 の後・08 の前に実施。06 / 07 は store.rs に触れない）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: core/findings.rs の既存テスト（HashMap join、caps、routing、sort）が
  移動後も全て green であることが振る舞い不変の証人。テスト側の変更は
  import 経路の更新のみに留める
- 依存方向: src/core 配下の grep で crate::store が 0 件であることを確認。
  store → core（src/store.rs:14-20）の正方向依存は維持される
- トリップワイヤ: tests/pure_join_placement.rs は既存の監視対象（trend の
  joins）について green を維持する。共有型の監視範囲拡張は本 PBI の
  スコープ外（別途検討）とし、本 PBI でトリップワイヤの定数や規約文を
  書き換えない
- 検証コマンド: cargo fmt --check / cargo clippy --all-targets --all-features
  -- -D warnings / cargo test

## 見積もり
0.5日（RICE の Effort と同一根拠。定義移動 + pub use 再エクスポート +
import 経路 2 箇所の更新のみで、SQL・シグネチャ・振る舞いは不変）

## Definition of Done
- [x] 全BDDシナリオが自動テストまたは機械的な確認手順としてパスする
- [x] diff が型定義移動のみであること（テスト名 diff 空）を確認済み
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・
  spec同期・干渉を目視確認。store.rs 排他の 06 / 07 / 08 との干渉を含む）
- [x] ドキュメント更新済み（architecture.md 等に core → store 逆依存に
  言及する記述があれば同梱更新。spec-sync 規約に従う）
