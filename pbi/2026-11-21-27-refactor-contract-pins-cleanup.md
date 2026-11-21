# PBI: 契約 pin の退化を防ぎ、語彙の対照を検証する

## ユーザーストーリー
メンテナとして、ハーネス一覧・種別語彙・adapter の doc・テストの抽出構造といった
暗黙契約が pin テストで守られ、退化したら黙らず赤で止まる状態がほしい。なぜなら、
これらの契約は既存テストが green を続けたまま静かに空洞化し、発見が「次の機能追加の
事故」（4th ハーネスの parse 拒否、新 kind の wedge 対象外取り残し、briefing からの
無音落ち）になるからだ。

## ビジネス価値
- 順位: 13 / 13 件中（最終、低群）。各項目は単独では小さいが、性質が同一
  （「契約が pin を持たない／pin が常に真で退行検出不能」）なので 1 本の清掃に
  統合する。7 項目の内訳と修正済み行番号は「実装者向け注記 → 現状コードの確認」。
- 保護対象: (1) 4th ハーネス追加時に `Harness::ALL` だけが取り残され parse /
  label_list / facts テストが新ラベルを静かに拒否する事故、(2) 新 kind が
  `KindTraits::NONE` で黙って全 wedge 対象外になる事故、(3) 共有 doc に反する
  adapter 実装のドリフト、(4)(5) 常に真の assert と構造 pin の黙った縮退、
  (7) briefing 変換でのフィールド無音落ち。
- 測定: 追加 pin の数と、各 pin を一時改変して赤になることの確認（mutation
  チェック）。挙動変更は pin 追加とテストヘルパ修正のみで、既存出力はバイト同一。

## BDD受け入れシナリオ

```gherkin
Scenario: ALL から取り残されたハーネスは pin が止める
  Given Harness enum と canonical 一覧 ALL が定義されている
  When  新しい variant を追加し ALL への登録を忘れる
  Then  網羅 pin が赤になり、取り残された variant が報告される

Scenario: 種別語彙の三重管理は対照テストが一致を強制する
  Given KIND_TRAITS に 9 種の語彙表がある
  And   アダプタが surface を構築する kind と usage が発行する surface_kind がある
  When  語彙対照テストを実行する
  Then  アダプタが生成する kind の集合は KIND_TRAITS と一致する
  And   usage の surface_kind はその部分集合である

Scenario: 裁定が必要な 2 項目は検討記録に裁定が残る
  Given KIND_TRAITS に producer を持たない measurable 行（mcp_tool / permission）がある
  And   file_event の doc と 2 アダプタの実装が乖離している
  When  実装者が各項目について選択肢と根拠を検討する
  Then  裁定と根拠が本 PBI 内の検討記録に記録される
  And   選択された側に pin または doc の修正が反映される

Scenario: 常に真だった assert は実態を検査するように修正される
  Given 残滓チェックが旧固定名を検査し常に真になっている
  And   doctor prose の抽出が最初の空行で黙って打ち切られる
  And   同名 AgentCost 2 型が field-by-field 変換で無音に接続されている
  When  各 pin を実態に合わせて修正する
  Then  対応する退行を一時的に起こすと各 pin が赤になる
  And   既存の出力はバイト同一のままである
```

## 受け入れ基準
- [x] `Harness::ALL` の網羅 pin が存在し、ALL から漏れた variant を pin（または
  コンパイラ）が検出する。strum 等の新依存追加はしない
- [x] 語彙対照テストが存在し、KIND_TRAITS の 9 種がアダプタが生成する kind の
  集合と一致し、usage の surface_kind がその部分集合であることを pin する
- [x] mcp_tool / permission デッド行について「表に残すか外すか」の裁定と根拠が
  本 PBI 内の検討記録に残され、裁定に沿って pin が更新されている
- [x] file_event doc の乖離について「doc を実態に合わせるか実装を揃えるか」の
  裁定と根拠が検討記録に残され、選択された側に一貫性がある
- [x] guide.rs の残滓チェックが実際に書かれる sibling 名（`.toml.tmp` 接尾辞）を
  検査し、残滓を残す退行で赤になる
- [x] doctor_prose_sanitized.rs の抽出がセクション構造の変化で黙って短くならない
  （非空の pin かつ次見出し終端などの構造的な終端条件を持つ）
- [x] store::AgentCost と optimize::AgentCost のフィールド名集合が一致することの
  pin が存在する（serde_json の key 比較など、手書き二重管理にしない）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green（既存出力はバイト同一、golden 系テストは無変更のまま green）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- tests/doctor_prose_sanitized.rs（バイナリ実行）— 抽出ヘルパの終端修正と
  非空 pin。doctor の prose 構造の退行を stdout 経由で検出する
- 既存 e2e_golden_harness / render_view_golden / advisor_language が
  「出力バイト同一」の証人。無変更のまま green であることを完了条件とする

### 統合テスト
- tests/fingerprint_restat.rs — ENV_LOCK の毒済み復帰。1 テストの失敗が
  残り 5 テストを連鎖パニックさせない（現状の env 直列化は維持・拡張しない）
- 語彙対照 pin は core (surface.rs) と adapter (config.rs / opencode.rs /
  usage.rs) をまたぐため、lib 内テストとして surface.rs tests に置く

### 単体テスト
- src/adapter/harness.rs tests — ALL 網羅 pin（ワイルドカードなしの網羅 match
  でコンパイラに variant 追加の訪問を強制させる形）
- src/core/surface.rs tests — KIND_TRAITS ↔ 生成 kind の対照、
  surface_kind ⊆ KIND_TRAITS
- src/cli/views/guide.rs tests — 残滓チェック修正（実際に sibling が残る退行で
  赤になることを mutation 確認）
- src/store.rs または src/core/optimize.rs tests — AgentCost フィールド一致 pin
  （serde_json::to_value の key 集合比較）

## 実装アプローチ
- **Outside-In**: E2E（doctor prose 抽出）から着手し失敗を確認してから内側へ。
  pin は「一時改変 → 赤 → 元に戻す → green」の mutation チェックで有効性を
  証明してから固定する
- **Red-Green-Refactor**: 各 pin を先に書き、改変で赤になることを確認してから
  本体は触らず固定する。裁定事項 2 件は pin を書く前に検討記録として確定させる
  （裁定が pin の形を決める）
- **リファクタリング**: 語彙対照 pin は現実の生成箇所を突き合わせるだけに留め、
  汎用列挙機構を作らない（落とし穴参照）

## 見積もり
1日。pin 追加 6 件 + テストヘルパ修正 2 件 + 検討記録で、本体コードへの
変更は doc 修正の可能性（裁定 B の結果）のみ。等価性の証人（golden 系）が
揃っているため小さく見積もる。

## 技術的考慮事項
- 依存関係: なし。strum 等 derive クレートの追加は依存変更になるため禁止
- テスタビリティ: 全 pin は mutation チェック（一時改変で赤）で有効性を確認する
- 非機能要件: 挙動変更は pin 追加とテストヘルパ修正のみ。既存出力はバイト同一。
  env を触るテストの直列化（ENV_LOCK）は現状を維持する
- 裁定事項 2 件（デッド行、file_event doc）は本 PBI 内に「検討記録」を残すこと
  を受け入れ基準に含める。どちらを選んでも既存出力はバイト同一に収まる選択肢が
  ある（doc 合わせは出力を変えない、デッド行の表外しは NONE 退避と同結果）

## 実装者向け注記

### 現状コードの確認
（着手前に必ず実行すること。行番号は 2026-11-21 時点・ブランチ 1121a で確認済み）

```bash
rg -n "pub const ALL" src/adapter/harness.rs
rg -n "static KIND_TRAITS" src/core/surface.rs
rg -n 'kind: "' src/adapter/ src/core/usage.rs
rg -n "struct AgentCost" src/
```

統合対象 7 項目の現状:

1. **ALL 網羅 pin なし** — `src/adapter/harness.rs:464` の
   `pub const ALL: [Harness; 3]` が enum の網羅性チェック外。4th ハーネス追加時、
   match はコンパイラが拾うが ALL だけ黙って取り残され、parse
   （harness.rs:511-515）・label_list（harness.rs:468-474）・facts テスト
   （harness.rs:893 と :919 の `for harness in Harness::ALL`）が静かに新ラベルを
   拒否する。ALL.len() を pin するテストは存在しない
2. **種別語彙の三重管理** — KIND_TRAITS（src/core/surface.rs:139-212、9 種）、
   adapter 側 kind リテラル（src/adapter/config.rs の構築子群 :38 skill /
   :106 hook / :182 rule / :247 command / :262 agent / :279 claude_md /
   :674 mcp_server、加えて src/adapter/opencode.rs:342 command / :358 agent）、
   usage 側 surface_kind リテラル（src/core/usage.rs:122 "agent" / :128
   "mcp_server"、hook 発火は src/adapter/transcript.rs:371）。相互検証テストなし
   — surface.rs:551-577 と :580-599 のテストは期待値を手書きで二重管理している
   （:557-567 の expected タプル、:587-598 のソート済みリスト）。新 kind は
   `KindTraits::NONE`（surface.rs:127-131）で黙って全 wedge 対象外で、その意図は
   doc（surface.rs:136-138）と unknown-kind テスト（surface.rs:601-611）が pin
   している。mcp_tool（surface.rs:172-179）と permission（surface.rs:180-187）は
   measurable=true の producer ゼロのデッド行 — 表に残すか外すかの裁定が必要
3. **file_event doc と 2 アダプタの乖離** — src/adapter/transcript.rs:667-672 の
   doc「basename を truncate 済み path から導出しない（cap は各形式に個別適用）」
   に対し、src/adapter/opencode.rs:187-196 と src/adapter/kilocode.rs:534-541 は
   まさに `basename(&truncate(raw))`（各 tool_target が opencode.rs:706 /
   kilocode.rs で truncate を返す）。claude 側（transcript.rs:673-680）だけが
   `clean_truncate(basename(raw))` で doc を守る。実害は限定的（store はハーネス
   毎に独立し cross-harness 集計は起きない、表示のみ）— doc を実態に合わせるか
   実装を揃えるかの裁定が必要
4. **常に真の残滓 assert** — src/cli/views/guide.rs:1591-1594（テスト
   `persist_config_round_trip_through_a_pid_unique_dir`、:1570）の残滓チェックが
   `path.with_extension("toml.tmp")`＝旧固定名 `config.toml.tmp` を検査するが、
   実装は guide.rs:516-518 の `{pid}.{nanos}.{seq}.toml.tmp` を書く。assert が
   常に真で退行検出不能
5. **構造 pin の黙った縮退** — tests/doctor_prose_sanitized.rs:29-37 の
   `doctor_routed_lines` が「WHAT TO FIX FIRST」後の最初の空行で打ち切る
   （:34 の `take_while(!is_empty)`）。doctor の items 出力
   （src/cli/views/doctor.rs:209-215、次セクション見出しは :228 の COST）の間に
   空行が入る変更で抽出が黙って短くなり、pin のカバレッジが減っても green が続く
6. **pin 指摘の修正（現状と不一致 → 読み替え）** — 元指摘「claude 側 2 テストが
   ENV_LOCK 外で CLAUDE_CONFIG_DIR を set_var」は現状のコードと合わない: 
   tests/fingerprint_restat.rs の全 6 テストが ENV_LOCK を取得済み（claude :70 /
   :106、opencode :165 / :186、kilocode :238 / :258）で、set_var（:76 / :112）は
   ロック保持下にある。残る非対称は毒済み復帰の有無のみ — 本ファイルは
   `.lock().unwrap()`（1 テストの panic で毒され残り 5 テストが連鎖 panic）、
   tests/analyze_template_parity.rs:22-26 は `into_inner()` で復帰する。毒済み
   復帰への統一のみを本 PBI の修正範囲とする（経緯は検討記録に残す）
7. **同名 AgentCost の二重定義** — src/store.rs:385-389 と src/core/optimize.rs:96-100
   の同名 `AgentCost` が field-by-field 変換（src/cli/mod.rs:698-706）で接続。
   store.rs:2019-2023 の `subagent_by_agent_briefing` が store 側を返す。store 側
   へのフィールド追加はコンパイルエラーにならず briefing から無音で落ちる

### 検討記録（実装開始時に記入する）
- 裁定 A（mcp_tool / permission デッド行）: 表に残す / 外す —
  選択と根拠をここに記す
- 裁定 B（file_event doc）: doc を実態に合わせる / 実装を claude 側に揃える —
  選択と根拠をここに記す
- 補記: 項目 6 の元指摘が現状コードと不一致だった経緯（毒済み復帰のみに縮小した
  理由）をここに記す

### 実装手順
1. 「検討記録」の裁定 A / B を先に確定させる（pin と doc の形が裁定に従う）
2. E2E から: tests/doctor_prose_sanitized.rs の `doctor_routed_lines` の終端を
   最初の空行から次の見出し（"COST"）ベースに変え、抽出非空の pin を足す。
   一時的に doctor.rs の items 間に空行を挟んで赤になることを確認してから戻す
3. 統合: tests/fingerprint_restat.rs のロック取得を毒済み復帰型に統一する
   （analyze_template_parity.rs:22-26 の `env_guard` 形を踏襲）
4. 単体: src/adapter/harness.rs tests に ALL 網羅 pin を足す。ワイルドカードなし
   の網羅 match で variant を列挙し、各 arm の variant が ALL に含まれ
   parse(label) で復元できることを assert する。一時的に ALL から 1 variant を
   外して赤を確認してから戻す
5. 単体: src/core/surface.rs tests に語彙対照 pin を足す — アダプタが生成する
   kind の集合（config.rs 構築子群 + opencode.rs 2 種 + usage.rs surface_kind）
   と KIND_TRAITS の一致、surface_kind の部分集合性。裁定 A に従い 9 種の期待
   集合を確定させる
6. 単体: src/cli/views/guide.rs の残滓チェックを実 sibling 名（親 dir の走査で
   `.toml.tmp` 接尾辞を探す）に修正し、残滓を残す退行で赤になることを確認
7. 単体: AgentCost フィールド一致 pin — serde_json::to_value で両型の key 集合を
   比較する（agent は Some で比較し None による key 落ちを避ける）
8. 最終ゲート: cargo fmt --check / cargo clippy --all-targets --all-features
   -- -D warnings / cargo test。golden 系（e2e_golden_harness /
   render_view_golden / advisor_language）が無変更のまま green であることを確認

### 落とし穴
- 項目 1 の網羅 pin は strum 等 derive 追加（依存変更）を避け、ワイルドカードなし
  の網羅 match で variant 列挙をコンパイラに強制させる形か、std::mem::discriminant
  を使う総当たりで書く。後者も variant の手書き列挙は避けられない点に注意
- 項目 2 の対照テストで「アダプタが生成する kind を汎用的に列挙する機構」を作らない。
  kind は Site 行パターンで 1 箇所ずつ（src/adapter/harness.rs:106-119 付近の
  Site 定義）に散っており、一般化は過剰設計になる。現実の生成箇所（config.rs の
  構築子群 + opencode.rs の 2 種 + usage.rs の surface_kind）を突き合わせるだけに
  留める
- guide.rs の `with_extension` は「最後の拡張子の置換」なので sibling は
  `config.{pid}.{nanos}.{seq}.toml.tmp` になる。stem は呼び出し毎に一意なため
  事後の名前復元は不可能 — 残滓チェックは親 dir の read_dir で `.toml.tmp`
  接尾辞を探すのが堅い
- doctor 抽出の終端を次見出しに変えるとき、セクション内には items の後の
  dropped 行（doctor.rs:217-224 の "… and N smaller recurring problem(s)"）も
  含まれる。既存の `stuck_line`（doctor_prose_sanitized.rs:41-45）と
  `assert_no_control_bytes` を壊さないこと
- AgentCost pin を serde_json で書くとき、`agent: Option<String>` が None だと
  key は残るが値比較が無意味になる。`agent: Some(...)` のサンプルで比較する
- 項目 6 は元指摘が現状不一致のため「ENV_LOCK 追加」の修正をしないこと。やるのは
  毒済み復帰の統一のみ。指摘の経緯を検討記録に残し、後続の反証で混乱しないように
  する

## Definition of Done
- [x] 全BDDシナリオが実証される（pin シナリオは自動テスト、裁定シナリオは
  検討記録の記入とその pin / doc への反映で）
- [x] 全 pin が mutation チェック（一時改変で赤になる確認）を通っている
- [x] 既存出力がバイト同一である（golden 系テスト無変更で green）
- [x] 変更が pin 追加・テストヘルパ修正・検討記録と doc 修正（裁定 B の結果）に
  収まっている
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（裁定 B で file_event doc を触る場合、
  docs/specs/session-format.md の該当記述と同期。該当がなければ更新不要と
  判断した）
