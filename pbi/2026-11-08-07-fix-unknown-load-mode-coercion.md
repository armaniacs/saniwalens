# PBI: 未知 load_mode の黙示的 OnDemand 強制を修正する

## ユーザーストーリー
saniwalens ユーザーとして、将来の harness や adapter が新しい load_mode
ラベルを書き始めても、未知ラベルが黙って OnDemand 扱いされず、警告と
明示的な経路付けで検知できるようにしたい。なぜなら現状は
`LoadMode::from_label(...).unwrap_or(OnDemand)` が未知ラベルを OnDemand に
強制し、`is_always_on` が StartupFull のみ真なので、その行は
always_on_heavy に到達し得ず、ウェッジの誤経路がラベル drift として
静かに溜まるからだ。

## 優先度
- 順位: 07 / 全17件
- RICEスコア: 6.4（Reach=4 / Impact=1 / Confidence=80% / Effort=0.5日）
- 根拠: ラベル drift 時のウェッジ誤経路を止める。Impact は低い
  （未知ラベルは現状存在しない）が、明示経路化のコストは小さい。

## BDD受け入れシナリオ
Scenario: 未知 load_mode ラベルは警告されカウントされる
  Given  カタログ行の load_mode が未知ラベル（例: `startup_preload`）である
  When  ユーザーが wedge の一覧（waste ビュー）を見る
  Then  その行は OnDemand として黙って扱われない
  And   未知ラベルが警告（stderr または集計）として可視化され、
  未知件数がカウントされる

Scenario: 既知ラベルの分類は変わらない
  Given  カタログ行の load_mode が既知ラベル（`startup_full`、`on_demand` 等）である
  When  ユーザーが wedge の一覧を見る
  Then  従来どおり分類される（`startup_full` の heavy 行は
  always_on_heavy に載り、`on_demand` 行は載らない）
  And   未知ラベルの警告は出ない

Scenario: 境界 — 空文字や大文字小文字違いは未知扱いである
  Given  カタログ行の load_mode が空文字または `Startup_Full` のような
  表記ゆれである
  When  ユーザーが wedge の一覧を見る
  Then  その行は既知ラベルに正規化されず、未知として警告・カウントされる

## 受け入れ基準
- [x] 未知 load_mode ラベルが黙示的 OnDemand に強制されない
  （明示的な経路付け: 警告 + カウント。既定の分類先を決めて固定する）
- [x] 既知ラベル 5 種の分類が現状どおりである
- [x] 空文字・表記ゆれの扱いを決めて固定する（未知として扱う）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: `LoadMode::from_label` の未知・空文字・表記ゆれが `None` であること。
  `config_wedges` が未知ラベル行を警告・カウント経路に回すこと。
  既知ラベル（特に `startup_full` の heavy 行）の always_on_heavy 到達が
  変わらないこと
- 統合: 未知ラベルを含む合成カタログで waste ビューに未知警告・カウントが
  出ること。既存 wedge の行が変化しないこと（capture の diff）
- E2E: 不要（core の純粋関数とビューの表示のみ）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状: src/core/findings.rs:120
  `LoadMode::from_label(&row.load_mode).unwrap_or(LoadMode::OnDemand)` が
  未知ラベルを OnDemand に強制する。src/core/surface.rs:62-64 の
  `is_always_on` は StartupFull のみ真なので、強制された行は
  findings.rs:125 の `load_mode.is_always_on() && ... HEAVY_TOKENS` 条件に
  到達し得ず、always_on_heavy から静かに漏れる
- 修正方向: silent coerce をやめ、未知 load_mode を warn + count して
  明示的に経路付ける（既定の分類先・警告の出し先・カウントの置き場所は
  実装時に決めて固定する）。`from_label` 自体のシグネチャ変更か、
  呼び出し側での `None` 分岐かは実装時に決める
- core は純粋に保つ（I/O フリー）。警告の出力先は cli 層に寄せるか、
  core が未知件数を返して cli が表示する形にする
- エビデンス: src/core/findings.rs（`config_wedges`、120 行目の coerce と
  125 行目の always_on_heavy 条件）、src/core/surface.rs（`LoadMode` enum、
  62-64 行目の `is_always_on`、67-76 行目の `from_label`）
- 関連コード: src/adapter/config.rs（load_mode ラベルの書き手側。
  将来の新ラベル追加時にこの PBI の経路に乗ることを確認する）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（surfaces.mdのcore契約 + cli.mdのwaste行）
