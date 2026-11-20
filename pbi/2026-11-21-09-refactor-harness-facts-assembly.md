# PBI: ハーネス静的コピー群を HarnessFacts 1 アームに集約する

## 優先度
- 順位: 09 / 全14件
- RICEスコア: 2.4（Reach=6 / Impact=1 / Confidence=80% / Effort=2日）
- 根拠: 4th harness（進行中 PBI 06、discovery 待ち）の実タッチポイントが、
  docs/specs/harness.md:254-266 の forward-compat 列挙（約 7 項目）に対して
  実測約 20 アームある。シェル側は seam データで動き無変更で済むが、静的コピー系
  アクセサが 15 本の独立 match メソッドに分散しており、次のハーネス PR は仕様を
  読んだだけでは readable_config・inventory_preamble・interactive_launch_command
  等を見落とす。copy() が既に 9 フィールドを集約していて集約パターンは実証済み
  （src/adapter/harness.rs:472-484）

## ユーザーストーリー
メンテナとして、新ハーネスの追加が 1 アームのデータ追加で完結し、仕様を読むだけ
で触るべき箇所が全部分かる状態がほしい。なぜなら静的コピー系アクセサが 15 本の
match メソッドに散らばったままでは、4th harness の PR が網羅 match の保護のない
静的コピー群で項目を取り漏らし、出力の破れがレビューの人手に依存するからだ。

## 背景
- `src/adapter/harness.rs` に静的コピー系アクセサが分散している
  （2026-11-20 時点の行。先行する PBI 07 でずれる前提で読むこと）:
  - :84-88 `Harness` バリアント / :442 `ALL` /
    :263-285 `catalog_surfaces` のアーム / :459-465 `analyze` — dispatch 系
  - :472-484 `copy()` — 9 フィールド集約の既存パターン（`HarnessCopy`）
  - :496-599 静的コピー 11 メソッド（`label`〜`input_unit`:
    label / display_name / article / instruction_file / project_config_ref /
    global_config_ref / source_noun / raw_source / config_inventory /
    readable_config / input_unit）
  - :609 `launch_command` / :639 `interactive_launch_command` /
    :657 `run_line_renderer` / :666 `inventory_preamble` /
    :696 `db_file_name`
- docs/specs/harness.md:254-266 の forward-compat 節は約 7 項目しか列挙しておらず、
  実タッチ全数を反映していない
- テスト側の既存の証人:
  src/adapter/harness.rs:837-944（parse・label・display・report copy・db 名・
  launch・renderer の 7 件）、src/cli/deliver.rs:470-545 付近
  （`DeliveryPlan::launch` の deliver 側テスト群）、
  src/cli/mod.rs:57-59（`--harness` の help 文言、本 PBI では変更しない）

## BDD受け入れシナリオ

```gherkin
Scenario: 既存 3 ハーネスの全出力がバイト同一のまま
  Given 静的コピー系 11 メソッドが HarnessFacts の 1 アームに集約された実装
  When  claude / opencode / kilocode の同一入力で各コマンドを実行する
  Then  すべての出力が集約前とバイト同一である
  And   --help 文言とストアの meta.harness 値も変わらない

Scenario: どのハーネスも facts() が同一フィールド集合を返す
  Given Harness::ALL の各ハーネス
  When  facts() を呼ぶ
  Then  返る HarnessFacts は同一のフィールド集合を持ち、全フィールドが非空である
  And   各値は集約前の対応アクセサの戻り値と一致する

Scenario: 新ハーネスの静的コピーは facts() 1 アームで済む
  Given dispatch 系（analyze / catalog_surfaces / run_line_renderer）が
  網羅 match のまま残っている
  When  Harness に新バリアントを追加する
  Then  静的コピーは facts() の 1 アーム追加で満たされる
  And   dispatch 系・facts() の未追加はコンパイルエラーとして検知される
```

## 受け入れ基準
- [x] 静的コピー系 11 メソッド（`label`〜`input_unit`）が HarnessFacts 構造体
  1 つに束ねられ、`facts()` の 1 アームに集約される（copy() パターンの踏襲）
- [x] dispatch 系（analyze / catalog_surfaces / run_line_renderer）は網羅 match の
  まま現状維持する
- [x] 既有 3 ハーネスの全コマンド出力が集約前とバイト同一である
  （golden / capture の diff が空）
- [x] 全ハーネスの `facts()` フィールド集合一致テストがある
- [x] docs/specs/harness.md の forward-compat 節に新ハーネスの実タッチ全数が
  列記されている（spec-sync に従う）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: 全ハーネスの `facts()` フィールド集合一致テストを新設する。
  値の一致は既存の src/adapter/harness.rs:837-944 のテスト群
  （display_names_name_the_harness / report_copy_names_each_harness_own_config /
  claude_keeps_the_historical_db_name など）が証人になる
- 統合: src/cli/deliver.rs:470-545 付近の deliver 側テストと
  tests/run_renderer_parity.rs が launch / renderer の seam 契約を固定する。
  本 PBI でこの契約が動かないことを既存テストの通過で確認する
- E2E: golden（tests/cli_help_golden.rs ほか）と capture diff で集約前後の
  出力バイト同一性を確認する。src/cli/mod.rs:57-59 の help 文言は本 PBI では
  変更しない

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- **着手順序（重要）**: 同日の PBI 07（ユーティリティ移動）が先行バッチで
  src/adapter/harness.rs を触るため、本 PBI は必ず **07 の後に着手する** こと。
  07 適用後は本ファイルの行番号がすべてずれるため、作業前に再確認する
- 集約対象は :496-599 の 11 メソッドのみ。launch / interactive_launch_command /
  inventory_preamble / db_file_name は本 PBI では match のまま残す
  （残す理由: 形が非均質で、dispatch・複合データを 1 構造体に押し込むと
  copy() パターンから外れる）。これらを含む実タッチ全数は spec の
  forward-compat 節に列記して補う
- HarnessFacts の置き場所は実装者が決めてよい（`core::optimize::HarnessCopy`
  と同列にするか adapter 内に置くか）。決定は spec-sync に従って spec に反映する
- 出力のバイト同一性がこのリポジトリの契約。新構造体導入に伴う表示差分は
  意図的変更として扱わず、差分が出たら実装側を寄せる
- エビデンス: docs/specs/harness.md（forward-compat 節と seam の規約）、
  copy() の集約パターン（src/adapter/harness.rs:472-484）、
  既存テスト（src/adapter/harness.rs の tests、src/cli/deliver.rs の tests、
  tests/run_renderer_parity.rs）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] 既有 3 ハーネスの出力バイト同一性が golden / capture diff で確認済み
- [x] docs/specs/harness.md の forward-compat 節が実タッチ全数で更新済み
- [x] コードレビュー完了（バッチ統合レビュー: 全diff・spec同期・PBI 07 との
  干渉を目視確認）
