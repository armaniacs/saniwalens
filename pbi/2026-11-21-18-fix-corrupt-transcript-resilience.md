# PBI: 不正 transcript 行をスキップして続行し、劣化を検出可能にする

## ユーザーストーリー
複数マシンで transcript を同期して analyze を回す saniwalens ユーザーとして、
1 行の破損（不正 UTF-8 バイト・usage の型不正）が混入しても analyze が
最後まで走り切り、どこをどれだけ諦めたかが可視化される状態がほしい。
なぜなら、計測ツールが 1 バイトの破損で恒久停止するのは可用性の実害であり、
黙ってデータを消すのは「劣化は検出可能であること」というプロジェクト契約の
裏切りだからだ。

## ビジネス価値
- 優先度: 順位 04 / 13 件中（analyze 恒久停止 = 計測ツールの可用性の実害）
- 根拠 1 — 不正 UTF-8 で analyze 全体が落ち、fingerprint と相まって恒久停止する:
  - `src/adapter/claude.rs:183-184` — 主 transcript の `fs::read_to_string` が
    不正 UTF-8 1 バイトで `Err`（dotfile 同期・部分書き込みで現実に発生）。
    `src/adapter/harness.rs:375` の `parse(candidate, pre)?` で analyze 全体が Err
  - fingerprint は ingest 成功後に `record_ingested_file` されるだけ
    （`harness.rs:397-411`、stamp 判定 :404-411）ため、失敗した候補はスタンプ無し
    → 毎回再試行 → 毎回失敗の恒久停止
  - 非対称: subagent 本体読みは `claude.rs:247-249` の
    `let Ok(text) = ... else { continue; }`、sidecar は :250-253 の `.ok()` で耐性あり
  - `src/adapter/kilocode.rs:820-823` も conversation/ui の `read_to_string` + `?`
    で同型の非耐性
- 根拠 2 — usage 型不正の行が無警告・無計上で消滅する:
  - `src/adapter/transcript.rs:124-130` — RawUsage の全フィールドが `Option<u64>`
    で、負数/小数/u64::MAX 超過があると serde が構造体全体の deserialization を失敗
  - :138-140 の `let Ok(raw) = ... else { return 0; }` で当該行の ToolUse/
    SkillInvocation も無警告・無計上で消滅。fingerprint スタンプ後は恒久
  - opencode（`opencode.rs:665-677` の `as_u64`→0 + sanitize :678-681）と
    kilocode（:198-204, :213-216）は行が生きる
- 根拠 3 — drop 計数の不一致（3 ハーネスで計数が揃っていない）:
  - claude は sanitize 経由の dropped 計上を持つ（`transcript.rs:204-209`）が、
    型不正は行ごと消滅して 1 つも数えられない（:138-140）
  - opencode/kilocode は型不正（負数/小数/u64 超過）を `as_u64().unwrap_or(0)`
    （`opencode.rs:669,676` / `kilocode.rs:203`）で 0 にするが drop 計上なし
  - 結果として `warn_dropped_usage_fields`（`transcript.rs:44-53`）の
    「counts the drops」契約が 2/3 ハーネスで過小報告
- プロジェクト契約: `transcript.rs:27-29` — 「劣化は検出可能であること」
  （degrades detectably）。deserialize 失敗形はこの契約の裏側

## BDD受け入れシナリオ

```gherkin
Scenario: 不正 UTF-8 バイトを含む transcript でも analyze が完走する
  Given 有効な行の間に不正 UTF-8 バイトを含む行が 1 行ある合成 claude transcript がある
  When saniwalens が analyze を実行する
  Then analyze はエラーなく完了する
  And 不正バイトを含む行だけがスキップされ、有効な行の記録は通常どおり計上される
  And stderr に 1 行のスキップ警告が出る（stdout には何も出ない）

Scenario: usage 型不正の行でも ToolUse 等が生き、drop として計上される
  Given usage に負数を含む assistant 行を持つ合成 claude transcript がある
  When saniwalens が analyze を実行する
  Then 当該行の ToolUse 記録は保持される
  And 当該行の usage は 0 扱いになり、drop として計上される
  And stderr に警告が出る
  And 2 回目の analyze で当該 transcript が再分析されない（スタンプは通常どおり付く）

Scenario: 型不正 usage の drop 計数が 3 ハーネスで一致する
  Given usage に負数・小数・u64 超過を含む合成 transcript が claude/opencode/kilocode それぞれにある
  When saniwalens が各ハーネスで analyze を実行する
  Then 3 ハーネスとも行は生き、usage は 0 扱いになる
  And drop 計数が 3 ハーネスで同じ扱いになり、stderr 警告がそれを報告する

Scenario: 正常な transcript の出力は 1 バイトも変わらない
  Given 現実的な範囲の usage 数値のみを含む正常な合成 transcript がある
  When 修正前と修正後のバイナリで同じ入力を analyze する
  Then e2e_golden_harness の 3 ハーネス golden がバイト同一である
  And stdout にも stderr にも警告が一切出ない
```

## 受け入れ基準
- [x] claude の主 transcript 読み（`claude.rs:183-184`）が不正 UTF-8 で
  analyze 全体を Err にしない。不正バイト行はスキップ + stderr 警告 1 行で、
  有効な行は通常どおり計上される
- [x] kilocode の conversation/ui 読み（`kilocode.rs:820-823`）が同様に
  analyze を落とさない（whole-JSON のため lossy 読み + 不正バイト検出時の警告）
- [x] claude の RawUsage 型不正行（`transcript.rs:124-130`, `:138-140`）が
  「usage 0 扱い + 当該行の ToolUse/SkillInvocation 保持 + drop 計上 + 警告」に
  統一される（行ごと消滅の解消。行スキップ方式は採らない）
- [x] opencode/kilocode の型不正 usage（負数/小数/u64 超過）が drop 計上され、
  3 ハーネスで計数の揃った扱いになる
- [x] fingerprint との整合を明示的に決定済み:
  `CLAUDE_ANALYZER_VERSION` を 10→11 に bump（`transcript.rs:70`。型不正行の
  復活が既存ストアの行に影響するため bump が必須）。OPENCODE/KILOCODE は
  行の生き死に・行内容が変わらず、変更が drop 計数（stderr のみ）と
  これまで読めなかったファイル（行を持たない）に限られるため bump 不要
- [x] 正常系出力が修正前とバイト同一である（e2e_golden_harness 3 ハーネス +
  正常系で stderr 空）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体（境界）: 不正 UTF-8 行の位置（先頭/中間/末尾）・複数行・全部不正、で
  スキップ数と残存記録を固定する。型不正 usage は負数/小数/u64::MAX 超過の
  各型で、usage=0 扱い・ToolUse 保持・drop 計数を 1 テスト 1 挙動で固定
- 単体（一致）: 同じ型不正値を 3 ハーネスのパーサに与え、drop 計数の扱いが
  揃うことを共通フィクスチャで確認する
- 統合: 不正 UTF-8 / 型不正を含む合成 transcript を analyze から store ingest
  まで通し、完走とスタンプ付与（2 回目は skip）を確認する
- E2E: 正常 transcript での修正前後の出力差分が空であること
  （e2e_golden_harness の既存 golden の diff）。毒入り fixture は合成のみ
  （実 transcript を持ち込まない）

## 実装アプローチ
- (a) claude: `fs::read_to_string` を bytes 読みに替え、行単位で
  `String::from_utf8` を試す（または `from_utf8_lossy` 前後でスキップ検出）。
  不正バイト行はスキップして数え、有効行だけを連結したテキストを既存の
  `parse_session` / `count_permission_denials` / `extract_*` 系に渡す。
  警告は `warn_dropped_usage_fields`（`transcript.rs:44-53`）と同じ stderr 専用
  ・正常系無出力の形のヘルパーにする
- (a) kilocode: conversation/ui とも whole-JSON 配列なので行単位読み替え不可。
  bytes 読み + `String::from_utf8_lossy` に替え、lossy が置換を起こしたら
  警告 1 行。JSON 構造が壊れていても parse_session が空を返すだけで
  analyze は完走し、警告で劣化が見える
- (b) claude `transcript.rs`: RawUsage を `Option<u64>` 直 deserialization から
  字段ごとの寛容パースに替える（例: `Option<Value>` を受けて
  `as_u64`/負数/小数を判別し、0 扱い + 既存の dropped 返却経路に流す）。
  行は生き続けるので ToolUse/SkillInvocation は既存経路で保持され、
  `current_prompt_id` / `reported_output` の行間処理も通常どおり流れる
- (c) opencode `:665-677` / kilocode `:198-204`: `as_u64().unwrap_or(0)` を
  「キーが数値として存在し `as_u64` が失敗 → 0 + drop 計上」に替え、
  既存の sanitize 経由の計数チャネルに合流させる
- analyzer version: `CLAUDE_ANALYZER_VERSION` を 10→11 に bump するのみ
  （bump 判断の根拠は受け入れ基準の通り）。core は変更しない — 正規化は
  アダプタ境界のみ（既存 PBI 02 と同じ原則）

## 見積もり
1日。claude/kilocode の読み替え + RawUsage 寛容パースが本体で、
3 ハーネスの drop 計数統一と境界テストが実質の作業

## 技術的考慮事項
- opencode は SQLite 読みで `read_to_string` が無く、(a) の該当外
- kilocode は conversation（`api_conversation_history.json`）と ui
  （`ui_messages.json`）の両方が対象。いずれも whole-JSON として
  `from_str::<Value>` で読まれる（`kilocode.rs:72-78`）ため lossy 一択
- claude の subagent 本体/sidecar 読み（`claude.rs:247-249, 250-253`）は
  既に耐性あり。本 PBI では主 transcript との警告形の対称性を崩さない範囲で
  現状維持（耐性付けの追加持々はしない）
- 正常系の証人は e2e_golden_harness（3 ハーネス）のバイト同一 golden。
  lossy/skip は不正バイトが存在する場合にしか挙動を変えないため、
  正常系の re-bless（UPDATE_GOLDEN=1）は発生しないはず
- excerpt/target（tool error / work event）は `extract_tool_errors` /
  `extract_work_events` の別系統パースから来るため、(b) の影響分離が可能。
  (b) でこれらの系統を触らないこと

## 実装者向け注記

### 現状コードの確認
実装前に以下の現行行番号を再確認する（本 PBI は 2026-11-21 時点で検証済み）:
- `src/adapter/claude.rs:183-184` — 主 transcript の `read_to_string` + `?`
- `src/adapter/claude.rs:247-249`（subagent 本体、`else { continue; }`）/
  `:250-253`（sidecar、`.ok()`）
- `src/adapter/harness.rs:368-374`（fingerprint skip 判定）/ `:375`
  （`parse(candidate, pre)?`）/ `:397-411`（ingest 後の stamp、
  `record_ingested_file` は :410）
- `src/adapter/harness.rs:808-817`（`file_fingerprint` = mtime nanos + size）
- `src/adapter/transcript.rs:27-29`（degrades-detectably 契約）/ `:44-53`
  （`warn_dropped_usage_fields` 契約）/ `:70`（`CLAUDE_ANALYZER_VERSION = "10"`）/
  `:124-130`（RawUsage）/ `:138-140`（parse_line の `else { return 0; }`）/
  `:204-209`（sanitize 経由の dropped 合算）
- `src/adapter/opencode.rs:40`（`OPENCODE_ANALYZER_VERSION = "6"`）/ `:665-677`
  （`number`/`cached` クロージャ）/ `:678-681`（sanitize）
- `src/adapter/kilocode.rs:46`（`KILOCODE_ANALYZER_VERSION = "6"`）/ `:72-78`
  （whole-JSON パース）/ `:198-204`（`number` クロージャ）/ `:213-216`
  （sanitize）/ `:820-823`（conversation/ui の `read_to_string` + `?`）

### 実装手順
1. まず赤いテストから: 不正 UTF-8 行を含む合成 claude transcript で
   analyze 完走 + スキップ警告、型不正 usage で ToolUse 保持 + drop 計数、
   3 ハーネス drop 計数一致、を失败する形で書く
2. claude の読みを bytes + 行単位 from_utf8 に替え、スキップ数を stderr
   ヘルパーで警告する（正常系は無出力）
3. kilocode の conversation/ui を bytes + lossy に替え、置換検出時に警告する
4. `transcript.rs` の RawUsage を寛容パースに替え、型不正を drop 計上チャネル
   （`parse_line` → `parse_session` → `warn_dropped_usage_fields`）に流す
5. opencode/kilocode の `as_u64().unwrap_or(0)` を drop 計上ありに替える
6. `CLAUDE_ANALYZER_VERSION` を "11" に bump し、緑を確認
   （golden diff 空、stderr 正常系空）

### 落とし穴
- 不正 UTF-8 行のスキップは「stderr 警告 1 行 + 無視」にする。
  既存の `warn_dropped_usage_fields` と同じ stderr 専用・正常系無出力の契約を
  維持すること（stdout に出すとビューのバイト同一性契約と衝突する）
- fingerprint との整合は必ず意識する: スキップしても ingest は成功するので
  fingerprint はスタンプされる = 「スキップされた行は version bump まで
  戻らない」。これは許容とする（劣化は警告で可視、元データの修復または
  analyzer bump で回収する）—— ただし (b) の型不正行は既存ストアに
  「行が消えた状態」でスタンプ済みのため、CLAUDE のみ bump が必須。
  bump すると既存ストア全体が 1 回再分析される（fingerprint 再分析）。
  OPENCODE/KILOCODE を誤って bump すると不要な全再分析が起きるので
  変更しないこと
- (b) で RawUsage を寛容にする際、serde の deserialization 失敗ではなく
  「字段ごとの解釈」に落とすこと。構造体全体を `Option<Value>` にすると
  既存の行間処理（prompt id / reported output）の型が崩れる
- lossy 読みは U+FFFD を JSON 文字列内に埋め込むため、文字列としては
  生きるが構造は壊れ得る。kilocode で「records が空 + 警告」は
  正しい劣化表現であり、解析を諦めて落とす扱いに戻さないこと
- core（span/metrics/usage）は触らない。正規化済みの値だけを流す
- `src/store.rs` は変更不要（本 PBI の修正は ingest 前の値と解析挙動のみに
  影響する）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` がすべて green
- [x] 正常系出力のバイト同一性を確認済み（e2e_golden_harness 3 ハーネスの
  golden diff 空 + 正常系で stderr 空）
- [x] `CLAUDE_ANALYZER_VERSION` 10→11 の bump と OPENCODE/KILOCODE 無変更を
  実装・テストで確認済み
- [x] ロールバック検討済み: version bump を伴うため、切り戻しは
  version 定数を戻すだけで fingerprint 再分析が走る（旧セマンティクスで
  再構築される）ことを確認済み
- [x] docs/specs（session-format*.md）へ不正行スキップ・型不正 usage の
  劣化検出の振る舞いを追記済み（spec-sync 規約に従う）
- [x] コードレビュー完了（spec 準拠 + コード品質）
