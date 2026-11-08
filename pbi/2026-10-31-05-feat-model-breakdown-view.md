# PBI: モデル別 breakdown ビューを追加する

## ユーザーストーリー
saniwalens ユーザーとして、モデル別の使用量（件数とトークン合計）を sql を
書かずに見るビューがほしい。なぜならどのモデルにトークンとコストが集中しているか
（強いモデルの常駐、安いモデルへの振り替え余地）を知り、設定の見直し先を
決められるからだ。

## 優先度
- 順位: 05 / 全28件
- RICEスコア: 6.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1日）
- 根拠: 既存列で完結する軽いビュー。feat の同点集団（06 の次ハーネス adapter）の
  先頭（Effort が最小のため）。

## BDD受け入れシナリオ
Scenario: モデル別に件数とトークン合計が降順で出る
  Given ストアに複数モデルのイベントがある（例: opus / sonnet / haiku）
  When  ユーザーがモデル別 breakdown のビューを実行する
  Then  モデルごとの件数とトークン合計が、トークン合計の降順で出る
  And   ウィンドウ（--since / --until / --last）を指定すると、
  そのウィンドウ内だけで集計される

Scenario: model が空の行は unknown として束ねられる
  Given ストアに model が記録されていないイベントがある
  When  ユーザーがモデル別 breakdown のビューを実行する
  Then  それらの行は unknown として 1 行に束ねて出る
  And   行を落としたり、別のモデル名に見立てたりはしない

## 受け入れ基準
- [x] 新ビューがモデル別の件数とトークン合計を降順で出す（modelsビュー。core bundle_modelsで決定順）
  （トークン合計が同じなら件数か名前で決定的な順序になる）
- [x] 集計は model 列を持つ既存テーブル（イベント側と subagent_runs 側）で完結する。（migrationなし。新規クエリ2件）
  schema の migration はしない
- [x] model が空（NULL）の行は unknown として束ね、行を落とさない
- [x] tool_error の model 列には起因ツール名が入る点に注意し（kindフィルタで除外。回帰テスト済み）、
  ビューの対象から除外するか分けて扱う（「モデル」として数えない）
- [x] 他のビューと同じ規約を持つ（--frozen/format/--db/freshness/framing/JSON名前付き。golden＋E2Eで固定）: --frozen / --format table|markdown|json / --db、
  freshness（自動 refresh）と freshness 行（stderr）、framing block（table のみ）、
  JSON は名前付きオブジェクト
- [x] --since / --until / --last を他のビューと同様に受け付ける（WindowedDispatch.open経路。E2Eで境界確認）（Period 節の規約に揃える）
- [x] docs/specs/cli.md の Views 表に行を追加し（architecture.mdのreport行も更新。view-landing規約どおりviews/着地）、このビューが担うロジック
  （unknown 束ね、起因ツールの分離、降順の決定性）が 1 行 sql を超える根拠を書く
  （spec-sync 規約: Views の curated 規約との整合）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: 合成ストアで table / markdown / json の各形式がモデル別行を降順で出すこと。
  unknown 束ねが json に現れること
- 統合: イベント側と subagent_runs 側の集計が分離して正しいこと
  （片側だけのデータで一方が 0 行になること）。freshness / framing / window の
  既存規約に従うこと
- 単体: 降順の順序（同点の決定性）、unknown 束ね、tool_error の除外、
  ウィンドウ境界

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- model 列は events（skill_invocation の代表モデル、NULL あり）と
  subagent_runs（NULL あり）に既存。sessions には model 列は無い（version のみ）ので、
  セッション単位のモデル集計が必要なら events 経由の集計になるか、別の判断になる。
  この PBI は既存列だけで完結させ、schema は変えない
- tool_error の model は起因ツール名（docs/specs/events.md の metrics 表）なので
  モデル集計に混ぜない。混ぜると「モデル別」という表示が嘘になる
- views の curated 規約（docs/specs/cli.md: ビューは 1 行の query では書けない
  ロジックを持つことで command を得る）との緊張に注意。unknown 束ねと降順だけでは
  薄いという批判があり得るため、spec に根拠を書くか、この PBI をビュー追加ではなく
  既存 usage ビューの per-model セクション追加に振る判断も可能。
  どちらにするかを spec 更新と一緒に決める
- ccusage の --breakdown が外部比較（ccusage はモデル別内訳を持つ）
- core は純粋関数のまま（集計の順序と束ねは core の usage 系に加え、I/O フリー）
- spec 更新: docs/specs/cli.md の Views 表（必要なら）。docs/specs/storage.md は
  変更なし（列追加しないため）
- エビデンス: docs/specs/cli.md（Views / Output formats / Period / Reporting
  honesty の各節）、docs/specs/events.md（metrics 表の model 定義、
  NULL if none qualifies）、docs/specs/storage.md（events.model / subagent_runs.model、
  sessions に model 無し）
- 関連コード: src/core/usage.rs（集計）、src/cli/mod.rs（ビュー追加の配線。新規ビューは
  src/cli/views/ に着地）、
  src/store.rs（events / subagent_runs の読み出し）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（E2E 3件＋core単体＋concentration＋help golden）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync: cli.md Views行＋architecture report行）
