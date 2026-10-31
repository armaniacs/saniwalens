# PBI: events.started_epoch に events_by_epoch インデックスを張る（planner の選択肢を宣言する）

## ユーザーストーリー
分析者として、ストアが育っても窓付きビューが速いままでいてほしい、なぜなら
利用者は蓄積を止められず、窓を狭めても現在は全行スキャンで時間が伸びるからだ。

## ビジネス価値
- ウィンドウ付きの全ビュー（8 ビュー + trend の 2 ウィンドウ）と doctor /
  optimize の集計が、行数の増加に対して線形のスキャンから索引探索に変わる
- 測定方法: 2026-10-31 のなぜなぜ分析の実測ベンチ（SQLite 3.54.0、合成
  fixture、7 日窓）で、1M 行の窓付きクエリが 106-220ms から 36-42ms に
  （約 3〜5 倍）。プランは `SCAN events` から `SEARCH USING INDEX` に変化
- **トリガーは行数で観測可能**: 現行の claude ストアは約 1.1MB（約数万行）で
  効果は体感以下。約 50〜100 万行（1M 行で trend 1 実行約 500ms / findings
  約 1s、体感閾値 100ms/操作を超える点）から効果が現れる。行数は
  `saniwalens sql "SELECT COUNT(*) FROM events"` で確認できる

## 背景（2026-10-31 のレビュー持ち越し指摘1、なぜなぜ分析の実証）
- インデックスは `INDEX_SCHEMA`（src/store.rs:107-111）の 3 本のみ
  （events_by_surface / subagent_runs_by_agent / subagent_runs_by_source）。
  `started_epoch` は events テーブル（src/store.rs:42）に列があるが無索引
- spec DDL も同一（docs/specs/storage.md:67, 90-91）。矛盾ではなく未述:
  index を「planner の選択肢」として管理する契約が無い
- events を走査する窓付きリーダーは 13 メソッド、`trend_aggregates`
  （src/store.rs:1285-1302）は 2 ウィンドウ x 4 クエリ、`findings_input`
  （src/cli.rs:1076-1119）は doctor / optimize 用に 12 本を 1 回で発行する
- ingest の `DELETE FROM events WHERE source_path = ?`（src/store.rs:772）は
  このインデックスでは速くならない（実測で不変。別変更粒度）

## BDD受け入れシナリオ

```gherkin
Scenario: 新しいストアにインデックスができる
  Given ストアがまだ無い
  When ユーザーが `saniwalens analyze` を実行する
  Then ストアに events_by_epoch が作られている
  And 窓付きビューは正しい結果を出す

Scenario: 既存のストアにも次回 open で作られる
  Given インデックス無しで作られた既存のストアがある
  When 読み書きモードでストアを開く（任意のコマンド）
  Then events_by_epoch が自動で作られ、SCHEMA_VERSION は変わらない

Scenario: インデックスの有無で結果は変わらない
  Given 同じデータのストアが 2 つある
  When 同じ窓付きコマンドを実行する
  Then 出力はバイト同一である
```

## 受け入れ基準
- [ ] `INDEX_SCHEMA`（src/store.rs:107-111）に
  `CREATE INDEX events_by_epoch ON events(started_epoch);` を 1 行追加する
- [ ] 既存ストアへは次回 ReadWrite open で自動作成される
  （`reconcile_derived`、src/store.rs:1738-1748 付近）。
  `SCHEMA_VERSION` bump は不要で、機能テスト・公開 API に影響しない
- [ ] spec 更新: `docs/specs/storage.md` DDL に同一の 1 行を追加し、
  「インデックスは planner の選択肢」の契約節（張る原則、events_by_epoch の
  根拠＝bench 実測、`events.source_path` は別変更である旨、ReadOnly open
  （`--frozen` / `sql`）では作られない旨、トリガー約 50〜100 万行）を
  同一変更で追記する
- [ ] README の Notes と `docs/specs/cli.md` Period 節に利用者向け一言を
  追記する（ストアは蓄積で育つ、行数の確認コマンド、`--frozen` / `sql` では
  未作成なら使われない）
- [ ] テスト: インデックス存在の検証、窓付きクエリの結果不変（インデックス
  有無でバイト同一）、in_memory でも green
- [ ] bench は含めない（pbi/2026-10-31-27-test-analyze-bench が別で依存。
  同時実装すると RICE が下がるため、別 PBI のまま依存で並べる）
- [ ] `just check` と `cargo test` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 新規 analyze のストアに events_by_epoch がある、窓付きビューの結果不変

### 統合テスト
- 既存ストアへの次回 open でインデックスが作られる（SCHEMA_VERSION 不変）

### 単体テスト
- `INDEX_SCHEMA` の内容と reconcile_derived の収束

## 実装アプローチ
- **Outside-In**: E2E（新規ストアのインデックス）から失敗を確認し、
  reconcile（統合）→ スキーマ定数（単体）の順に Red-Green-Refactor
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=0.5〜1日。RICE 6.0: R=2 / I=1 / C=3 / E=1日。**I=1 の値とトリガー
（約 50〜100 万行）をこの行に明記し、行数がトリガーに達した時点で採点し直す
（I=2〜3 になれば RICE 12.0〜18.0）**）

## 技術的考慮事項
- 依存関係: bench は pbi/2026-10-31-27-test-analyze-bench（別 PBI、依存）。
  本 PBI は bench 無しで完結する（効果の根拠は 2026-10-31 の実測ベンチ）
- テスタビリティ: `reconcile_derived` は既存の移行経路、合成 fixture
- 非機能要件: プライバシー（合成 fixture のみ）、出力不変

## 実装者向け注記

### 現状コードの確認（2026-10-31 実施済み）
- `src/store.rs` の `INDEX_SCHEMA`（107-111 行目付近）: 3 本の定数配列。
  ここに 1 行追加する
- `reconcile_derived`（1738-1748 行目付近）: 既存ストアへの派生オブジェクト
  作成を担う。新インデックスもここで作られる
- `docs/specs/storage.md` の DDL（67, 90-91 行目付近）と spec の一致を
  spec-sync 規約で確認する

### 実装手順
1. `INDEX_SCHEMA` に 1 行追加する
2. テストを追加（インデックス存在、結果不変、既存ストアへの自動作成）
3. `docs/specs/storage.md` の DDL と契約節を更新する
4. README と cli.md に一言を追記する
5. `cargo test` → `just check`（直接実行）→ 出力不変を確認

### 落とし穴
- 若い DB では planner が `events_by_surface(surface_kind = ?)` を選び続ける
  ため、効果は行数と統計が増えて初めて実現する（実測のとおり）。
  ベンチの主張を spec に書くときは「行数が増えてから効く」ことを明記する
- ReadOnly open（`--frozen` / `sql`）ではインデックスは作られない非対称が
  残る。これを直さず、README と spec に明記する（挙動の契約として）
- `end + 1`（src/store.rs:615 付近）の `checked_add(1)` 化は任意の堅牢性
  改善（現行入力経路では到達不能）。本 PBI の本体ではない

## Definition of Done
- [ ] 全 BDD シナリオが自動テストとして実装されパスする
- [ ] `just check` と `cargo test` が green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] docs/specs/storage.md、README、docs/specs/cli.md 更新済み
