# PBI: サーフェス種別の性質述語を core に集約する

## 優先度
- 順位: 03 / 全14件（pbi/2026-11-21-00-backlog-holistic.md のバッチ1。01 / 02 / 04 と並列、ファイル排他なし）
- RICEスコア: 8.0（Reach=4 / Impact=1 / Confidence=100% / Effort=0.5日）
- 根拠: 種別の振る舞い（使用計測可能か・説明を目的比較に使えるか・never-fires 対象か）が
  複数箇所の別々の文字列リスト/条件に散っており、新しい種別を追加しても
  wedge 対象から黙って漏れる。レイヤ規約上、種別セマンティクスは core に寄せるのが
  本来の位置。依存はなし。

## ユーザーストーリー
saniwalens を拡張するメンテナとして、サーフェス種別の性質（使用計測可能か・
説明を持つか・never-fires 対象か）が core の 1 箇所で答えられる状態がほしい。
なぜなら今は種別ごとの判定が core のハードコード集合と view 層の文字列リストに
散らばっており、種別を 1 つ足すたびに複数ファイルを探し回り、足し忘れても
テストは黙って green のままだからだ。

## 背景（散在の実証・行番号は 2026-11-21 時点）
- 使用計測可能か: src/core/surface.rs:120-125 の `is_usage_measurable` が
  `"skill" | "agent" | "mcp_server" | "mcp_tool" | "permission" | "hook"` を
  ハードコード。テスト `rules_and_claude_md_are_catalog_only` が
  src/core/surface.rs:417-421 でピンする。呼び出し側は
  src/cli/views/waste.rs:59・:224・:262 と src/core/findings.rs:184・:325。
- redundant 対象: src/cli/views/waste.rs:144 が `"skill" | "agent" | "command"`
  を view 層で再綴りしている。
- never-fires 対象: src/cli/views/waste.rs:98 が `e.kind == "rule"` を view 層で
  判定している（本体の `never_fires` は core だが、対象種別の選別は view にある）。
- 種別一覧の doc: src/core/surface.rs:83 の `Surface::kind` doc コメント。
  アダプタが実際に生成する `command`（src/adapter/harness.rs:172-174 の
  `load_commands`）がここには載っておらず、一覧の陳腐化は既に起きている。
- 観測側の対応する写像: src/store.rs:886-892 が usage イベントの surface_kind →
  イベント kind の if 連鎖（`agent`→`agent_spawn` / `hook`→`hook_use` /
  else `tool_use`）を持つ。これは観測の符号化であり、本 PBI では変更しない。
- adapter 側のデータシームは完成済み: 新しい種別は Site 行とローダで登録でき、
  トラバーサルは無変更で済む（src/adapter/harness.rs:101-141、テスト
  `a_new_surface_kind_registers_as_a_site_row` は src/adapter/harness.rs:1043）。
  残った伸びしろは core 側の種別セマンティクス。

## BDD受け入れシナリオ

```gherkin
Scenario: 述語を集約しても wedge の振る舞いは変わらない
  Given 集約前の cargo test が green で、waste / briefing の現行出力を pin してある
  When  waste.rs の 2 つの種別フィルタを core/surface.rs の述語へ移すリファクタを適用する
  Then  cargo test が green のままである
  And   同じ合成カタログに対する waste / briefing の出力が集約前とバイト同一である

Scenario: 種別の性質は core の 1 箇所から答えられる
  Given core/surface.rs に種別ごとの性質を答える述語がある
  When  各種別について使用計測可能か / 説明を redundant 比較に使えるか / never-fires 対象かを問い合わせる
  Then  答えは集約前の view 層フィルタ（waste.rs の never-fires 選別と redundant 選別）の判定と一致する
  And   waste.rs と findings.rs は種別の文字列リストを自前では持たない

Scenario: 既存の境界ピンは壊れない
  Given rule と claude_md は catalog-only、hook は計測可能という既存ピンがある
  When  集約後のテストを実行する
  Then  rules_and_claude_md_are_catalog_only が改変なしで green である
```

## 受け入れ基準
- [x] waste.rs の never-fires 種別選別（:98）と redundant 種別選別（:144）が
  core/surface.rs の述語（`has_described_purpose` / `supports_never_fires`、
  `is_usage_measurable` の隣）に置き換わり、view 層から種別の文字列リストが消える
- [x] 余裕があれば 3 述語を 1 つの KIND_TRAITS 表（measurable / describable /
  globbable）に統一し、次の種別追加が core 1 ファイルの 1 行で済む。
  統一しない場合は 3 述語を `is_usage_measurable` の隣に並べ、doc で相互参照する
- [x] src/store.rs:886-892 の kind 写像は変更しない（観測の正常系として現状維持）
- [x] 既有 wedge 出力のバイト同一性を pin するテスト（または既存 golden）が green
- [x] 種別セマンティクスの単体テストが src/core/surface.rs に集約される
  （3 述語それぞれの真偽表）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: src/core/surface.rs に既知の全種別（skill / agent / mcp_server / mcp_tool /
  permission / hook / rule / claude_md / command）について 3 述語の期待値を表で pin。
  未知種別はすべて false（種別が漏れたとき静かに対象外になる現行の意味論を維持）
- 統合: 合成カタログ（各 wedge が 1 件以上出る fixture）で waste の JSON / テーブル
  出力が集約前と同一。findings の briefing 経路（`is_usage_measurable` の呼び出し側
  src/core/findings.rs:184・:325）も同一
- E2E: 既存の cargo test スイート全体がピンとして機能する。リファクタなので新規
  E2E は足さず、現行出力 pin を先に固定してから手を入れる

## 見積もり
0.5日（RICE の Effort と同一根拠。述語移設と真偽表テスト、出力同一性確認を含む。
KIND_TRAITS 統一は同日内の余裕判断）

## 実装者向け注記
- 移設先: `is_usage_measurable`（src/core/surface.rs:120-125）の直下に
  `has_described_purpose(kind)` / `supports_never_fires(kind)` を並べる。各述語の
  doc は「どの wedge が消費するか」まで書く（Redundant / NeverFires）。
  KIND_TRAITS 表に統一する場合も `is_usage_measurable` をラッパーとして残し、
  既存呼び出し側（waste.rs:59・:224・:262、findings.rs:184・:325）のシグネチャを
  変えない
- 現状の付随事実: `is_usage_measurable` と `HEAVY_TOKENS` の doc コメントが
  src/core/surface.rs:110-119 で相互に食い込んでいる（2 つの doc が継ぎ目で混線し、
  const をはさんで文が分断されている）。この領域を触る際に文を復元して直してよい
- view 側の書き換えは filter の差し替えのみ: waste.rs:98 を
  `supports_never_fires(&e.kind)` へ、waste.rs:144 を `has_described_purpose(&e.kind)`
  へ。redundant は description の有無でさらに絞る（waste.rs:145-153）ため、述語は
  「種別として説明を持ちうるか」を答える。description が None の行は現行どおり対象外
- spec 同期: docs/specs/config-format-opencode.md:73 が `is_usage_measurable` を
  名前で引用している。関数名・構成を変えた場合は .claude/rules/spec-sync.md に従い
  当該箇所を同梱更新する。docs/specs/surfaces.md:57-62 の種別×計測可能性表は
  意味論の唯一の記述なので、KIND_TRAITS 化する場合は表と述語の対応を doc で結ぶ
- store.rs は触らない: src/store.rs:886-892 の写像はイベント観測の符号化であり、
  wedge 対象の意味論ではない
- 検証コマンド: cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test
- privacy: 本 PBI は新規入力を読まない。テスト fixture は合成のまま
  （.claude/rules/session-data-privacy.md）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
