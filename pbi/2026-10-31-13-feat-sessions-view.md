# PBI: sessions ビュー（セッション一覧）を追加する

## ユーザーストーリー
saniwalens ユーザーとして、コスト最大・長時間・friction 多めのセッションを一覧で見る機能がほしい、なぜならどのセッションが重いかを sql コマンドなしで一目で判断でき、次に見直すべき対象の起点にできるからだ。

## 優先度
- 順位: 13 / 全28件
- RICEスコア: 4.0（Reach=2 / Impact=1 / Confidence=3 / Effort=1.5日）
- 根拠: 既存の sessions テーブルで完結する軽いビューで、スキーマ変更も入力解析の追加も不要のため確信度が高い。依存はなく単独で着手できる。

## BDD受け入れシナリオ
Scenario: 指定した基準で上位のセッションが降順に表示される
  Given ストアに複数のセッションが記録されており、セッションごとにトークン合計・所要時間・friction の回数が異なる
  When  ユーザーが基準を指定してセッション一覧を表示する
  Then  指定した基準の値が大きいセッションから降順に一覧が表示され、上位に重いセッションが現れる
  And   各行にはセッションの識別情報（開始時刻・プロジェクト）と、基準の値が載る

Scenario: friction が空のセッションも一覧に現れる
  Given friction（失敗や行き詰まり）が 1 件も無いセッションがストアに存在する
  When  ユーザーがセッション一覧を表示する
  Then  friction が空のセッションも、他の基準の値とともに一覧に現れる
  And   friction の値は 0 と示され、欠落として隠されない

## 受け入れ基準
- [x] セッション一覧のビューが追加され、セッションごとにトークン合計・所要時間・friction の回数を表示する（sessionsビュー＋store.session_breakdown）
- [x] 並び替えの基準（トークン合計・所要時間・friction）を指定でき、既定はトークン合計の降順、上位が最初に表示される（--sort。同値id決着で順序固定）
- [x] friction が空のセッションも一覧に現れ、friction は 0 と示される（E2Eで固定）
- [x] 各行にセッションの識別情報（開始時刻・プロジェクト）を載せる（started_at/project/slug/id）
- [x] 出力形式は他のビューの慣例（table / markdown / json）に揃える（help golden＋E2Eで固定。jsonは名前付きオブジェクト）
- [x] 既存ビューと sql / analyze / optimize の出力は一切変わらない（追加のみ。全greenで確認）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: 複数セッションの合成ストアで、基準指定ごとに降順の上位が表示され、friction が空のセッションも現れることをユーザーシナリオとして確認する
- 統合: sessions テーブルと events の集計（トークン合計・所要時間・friction の回数）が正しく読み出され、並び替えが同値・空値の境界でも崩れないことを確認する
- 単体: 並び替えの比較（基準ごとの降順・同値の扱い）、friction 空 0 扱い、セッションが無いストアでの表示をテストする

## 見積もり
1.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- sessions テーブルは既存（src/store.rs 22 行目付近： id・project・slug・root・source_path・started_at・sub_tokens・sub_agent_count）。セッションのトークン合計・所要時間・friction は events の join から集計する（src/store.rs 263 行目付近の JOIN を参照）
- コマンドは src/cli/mod.rs の Command enum（67 行目付近）に追加する。既存ビューの流れ（parse_period → Session::open_read → 集計 → 行、freshness 表示）の慣例に従う
- core は純粋関数のままにする（並び替えと行組み立ては core か report 側の純粋関数に置き、I/O は cli/mod.rs / store.rs に集約）
- spec 更新: docs/specs/cli.md の Views 表に新しいビューの行を追加する（動作変更と同じ変更内で）。storage.md はスキーマ変更が無ければ触らない
- セッション軸（sessions.started_at）とイベント軸（events.started_epoch）の二軸問題に注意。期間指定を持つ場合は PBI 01（pbi/archived/2026-10-30-01-feat-period-filter.md）のテスト規約に揃える
- TDD: e2e（合成ストア経由）→ 統合（集計と並び替え）→ 単体（比較・境界）の順に赤緑リファクタ
- プライバシー: テストは合成 fixture のみ。実 transcript はコミットしない

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（E2E 3件＋単体。JSON goldenを含む）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（cli.md Views行＋architecture report行。view-landing規約どおりviews/着地）
