# PBI: ログ打ち切りの plan を視認可能にする

## ユーザーストーリー
`--apply` で headless アドバイザの plan を対話セッションに引き継ぐ saniwalens ユーザーとして、
生ログが PLAN_MAX_BYTES（64KB）で打ち切られた場合でも、ダイアログ session が読む
plan ファイル自体に「打ち切られた」印が付いてほしい。なぜなら stderr の警告 1 行は
流れて見逃すことがある一方、plan ファイルはダイアログ session が全文を読み込む成果物で
あり、無印のままでは途中で切れた plan を advisor の完全な plan と誤認してそのまま
適用してしまうからだ。

優先度: 順位 10 / 13 件中（adversarial-review 発見。反証では未検証のため、本 PBI は
観測から始める）

## ビジネス価値
- handoff の契約「a cut is always visible and never mistaken for the advisor's complete
  plan」（src/cli/handoff.rs:165-168）が plan-size 側でしか守られていないと指摘されている
  穴を、plan-size と log-size の両側で閉じる
- 誤適用リスクの低減: 切れた plan を完全な plan と誤認して apply する事故を防ぐ
- 観測主導の着手: 指摘が誤り（既に marker が付く）なら観測結果を pin テストで固定して
  即完了でき、無駄な実装を入れない

## BDD受け入れシナリオ

シナリオは現時点の想定で書いてあり、観測フェーズの実測に合わせて確定する
（実装者向け注記参照）。

```gherkin
Scenario: 打ち切りのないログでは plan ファイルは無印のまま保たれる
  Given Text 行の合計が PLAN_MAX_BYTES 以内の headless 実行ログがある
  When --apply の handoff が plan を抽出して保存する
  Then plan ファイルの内容は既存実装とバイト同一である
  And plan ファイルに打ち切りマーカーは現れない

Scenario: 生ログの打ち切りが plan ファイルに視認可能に引き継がれる
  Given --timeout 0 で動く多弁なアドバイザが PLAN_MAX_BYTES を超える生ログを残した
  When capture_headless_log が読み込みを打ち切り handoff が plan を保存する
  Then plan ファイルの末尾に PLAN_TRUNCATION_MARKER 相当の印が付いている
  And 生ログの打ち切りが stderr に明示されている
  And handoff 自体（dialog 起動まで）は続行する

Scenario: 観測の結果 marker が既に付いていることが判明した場合
  Given 生ログが PLAN_MAX_BYTES で打ち切られる合成 fixture を handoff に通した
  When plan ファイルの内容と truncate_plan の返り値と stderr 出力を実測する
  Then 実測が「既に marker が付く」を示すなら pin テストを追加して本 PBI は完了する
  And 実測が「無印のまま」を示すなら観測結果を記録して打ち切りの視認化を実装する
```

## 受け入れ基準
- [x] 観測フェーズの結果が本 PBI に記録されている: 64KB 打ち切りになる合成ログでの
  (a) 実際の plan ファイル内容 (b) truncate_plan の返り値 (c) stderr 出力の実測
- [x] 観測が「無印のまま」を示した場合: 生ログが打ち切ったことが extract_plan_text
  完了後の plan 組立まで運ばれ、plan ファイル末尾に PLAN_TRUNCATION_MARKER 相当の
  印が付く（または plan_truncated_note を stderr に加えて明示する）
- [x] 観測が「既に marker が付く」を示した場合: その挙動を固定する pin テストが
  追加され、本 PBI は実装なしで完了扱いになる
- [x] 正常系（打ち切りなし）はバイト同一で、既存 handoff テストが無変更で green
- [x] 生ログ打ち切りの合成 fixture は「Text 行が打ち切り位置で切れる」「打ち切り後に
  Text が全く来ない」の 2 種をカバーする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体: 生ログ打ち切りフラグの伝播（read_capped の返り値 → plan 組立まで）の境界。
  64KB ちょうど（打ち切りでない）・1 バイト超過・UTF-8 文字境界またぎを固定する。
  観測で「既に marker が付く」なら、代わりに現挙動を固定する pin テストが単体になる
  （t_wada: テストは観測した仕様を固定する — Red になるかどうかは観測が決める）
- 統合: `complete_handoff` の double テストで「log 打ち切りフラグ付きの入力」→ plan
  ファイルが marker 付きになり、dialog 起動まで完遂すること。正常系のバイト同一性は
  既存 handoff テスト全体（`complete_handoff_saves_text_only_and_launches_the_dialog`
  等）が無変更 green の証人
- E2E: capture_headless_log の合成 advisor（実 spawn + 合成 JSON、既存 fixture 形）で
  64KB 超過ログを作り、(a) plan ファイル内容 (b) truncate_plan の返り値 (c) stderr
  出力を実測する。観測フェーズそのものがこの E2E に相当する

## 実装アプローチ
観測フェーズを最初に置く。hung/多弁 advisor を想定した合成 advisor で log が 64KB
打ち切りになった場合の (a) 実際の plan ファイル内容 (b) truncate_plan の返り値
(c) stderr 出力を実測し、BDD を実測に合わせて確定してから実装する。

実測が「打ち切った plan に marker/note が付かない」を示すなら: capture_headless_log
が打ち切ったことを extract_plan_text 完了後の plan 組立まで運び（read_capped の
bool を run_apply 経由で complete_handoff へ渡す）、plan ファイル末尾に
PLAN_TRUNCATION_MARKER 相当を書く。または plan_truncated_note を stderr に加えて
明示する。実測の結果「既に marker が付く」なら、その pin テストを追加して PBI を
即完了扱いにする。

## 見積もり
0.5日（観測 0.25日 / 実装+テスト 0.25日を含む）

## 技術的考慮事項
- 正常系（打ち切りなし）はバイト同一。ロールバック不要
- 打ち切りフラグを complete_handoff まで渡す場合、Handoff 構造体への追加か引数追加
  になる。dialog 側の挙動（apply prompt・起動コマンド）は変わらない
- 空 plan チェック（src/cli/handoff.rs:280-285 付近）は truncate_plan（:286）より前に
  あるので、打ち切りのためにスキップされることはない（先行 PBI 2026-11-21-11 の
  反証で確認済み）
- stderr の log_truncated_note（src/cli/handoff.rs:201-207）は読み込み時点で 1 回出る。
  plan ファイルへの marker 追加はこれを置き換えるものではなく補完する
- marker を plan に書く場合、既存の PLAN_TRUNCATION_MARKER（src/cli/handoff.rs:168）を
  再利用し、plan-size cut と log-size cut の両方が効いたときに marker が 2 重付与に
  ならないようにする

## 実装者向け注記

### 現状コードの確認 — この PBI は観測から始める
発見指摘は adversarial-review 出典で反証では未検証。まず観測する。

- read_capped（src/cli/handoff.rs:215-242）は生ログの読み込みを 64KB
  （PLAN_MAX_BYTES、:163）で打ち切り、(String, bool) を返す
- capture_headless_log（src/cli/handoff.rs:320-351、read_capped 呼び出し :343）は
  log_truncated なら stderr に log_truncated_note（:201-207）を出すだけで、フラグは
  それ以上使われず complete_handoff には伝わらない
- complete_handoff（src/cli/handoff.rs:265-313）は読んだテキストから
  extract_plan_text（src/adapter/harness.rs:48-57、`Text` 行のみ stream 順 join）で
  plan を作り、truncate_plan（:174-186）を :286 で適用する。打ち切られた log から
  作られた plan は 64KB 未満に収まるため truncate_plan は (plan, false) を返し、
  PLAN_TRUNCATION_MARKER（:168）も plan_truncated_note（:191-197）も出ない
- その結果、save_plan（src/cli/handoff.rs:131-136）が書く plan ファイルは無印のままに
  なり、ダイアログ session が読むのは切れたことに気付けない plan テキストだと
  指摘されている — 契約「a cut is always visible and never mistaken for the
  advisor's complete plan」（:165-168）が plan-size 側でしか守られていないという主張
- 観測で確認するもの: (a) plan ファイルの実際の内容（marker の有無）(b) truncate_plan
  の返り値 (c) stderr 出力。観測結果を本 PBI に追記し、BDD を確定する

### 実装手順
1. 観測: 合成 advisor（`/bin/sh` + 合成 JSON、handoff.rs 既存テストの fixture 形）で
   64KB 超過の生ログを作り、capture_headless_log → complete_handoff の経路で
   (a)(b)(c) を実測する
2. 観測結果を本 PBI に追記し、BDD シナリオを実測に合わせて確定する
3. 「無印のまま」なら: read_capped の打ち切りフラグを plan 組立まで運ぶ
   （run_apply 経由で complete_handoff へ）。extract_plan_text 完了後の plan に
   PLAN_TRUNCATION_MARKER 相当を末尾に書くか、plan_truncated_note を stderr に
   加えて明示する。TDD: 伝播フラグの境界テスト → Red → Green
4. 「既に marker が付く」なら: その挙動の pin テストを追加して PBI を完了扱いにする
5. 検証: cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
   cargo test

### 落とし穴
- log 打ち切りの合成 fixture は 2 種: (1) Text 行が打ち切り位置で切れるケース —
  切れた JSON 行は renderer で Silent になる（src/adapter/opencode.rs:1818-1823 の
  `serde_json::from_str` 失敗 → `RunEvent::Silent`）ため、その行のテキスト全体が
  plan から消える (2) 打ち切り位置の前後で Text が全く来ないケース — plan の末尾が
  静かに欠ける。両方で現行挙動を実測すること
- 空 plan チェック（handoff.rs:280-285 付近）は truncate_plan（:286）より前なので、
  打ち切りのためにスキップされない（先行 PBI 2026-11-21-11 の反証で確認済み）
- marker を plan ファイルに書く場合、PLAN_TRUNCATION_MARKER を再利用して文言を
  増やさない。また log-size cut と plan-size cut が同時に効く入力（打ち切った head
  だけでも plan が 64KB を超える）で marker が 2 重に付かないようにする
- stderr は capture 時点で log_truncated_note が 1 回出ている。plan 保存時点の追加
  note を出す場合、同じ警告が 2 回見える構成にならないよう文言を整理する

## Definition of Done
- [x] 観測結果が本 PBI に記録され、BDD シナリオが実測に確定している
- [x] 全BDDシナリオが自動テストとして実装されパスする（観測で「既に marker が付く」
  の場合は pin テストで代替し、実装は不要と記録する）
- [x] 正常系（打ち切りなし）のバイト同一性が既存 handoff テストの無変更 green で
  証明されている
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] ドキュメント更新済み（docs/specs/cli.md の handoff セクションに log 打ち切り →
  plan への視認性を追記、spec-sync 該当があれば）
