# PBI: 時刻差分の算術を飽和化する

## ユーザーストーリー
saniwalens のレポートで skill span の所要時間と idle 判定を信じて設定を直す
ユーザーとして、他のマシンから同期された破損・改変 opencode データベースが
混入しても、時刻計測が嘘の値を出さず analyze が最後まで走り切る状態がほしい。
なぜなら計測ツール自身が入力を信頼して減算を overflow させ（Debug/テストでは
パニック、release では静かな wrap）、span の開閉境界を壊すのは、usage 計測と
同じく分析の完全性とツールへの信頼を根底から崩すからだ。

## ビジネス価値
- 優先度: 順位 03 / 13 件中
- 根拠: `src/core/span.rs:119` の
  `record.timestamp_ms - records[index - 1].timestamp_ms` が生の i64 減算。
  先行する `||` の短絡は現在レコードが HumanTurn | SkillInvocation のときしか
  効かず、それ以外（Assistant / ToolUse / AgentSpawn / Other）では必ず減算が
  評価される。opencode は part JSON の `state.time.start` を `as_i64` 素通し
  （`src/adapter/opencode.rs:204-212` 付近の `part_time`、:269 / :282 / :310 が
  DB から生 i64）なので、同一 assistant メッセージ内に skill part の
  `i64::MIN` と turn_end の `i64::MAX` を置くとソート後隣接ペアになり、
  debug で panic、release で wrap（span 境界破壊）する。同じ毒が
  `src/core/metrics.rs:36`（duration_sec の last - first）にも届く
- span.rs:202-205 付近のコメント「the core must still never wrap or panic on
  the values it is handed」はプロジェクト契約。PBI 02（usage 飽和化）と同じ
  信頼境界の未完了分であり、本 PBI で時刻軸を閉じる
- 壊れ方の重大さ: release の wrap は巨大な idle ギャップを微小な差分に偽装し、
  span が閉じなくなる（残りセッション全てを span が呑み込む）。計測ツールと
  して「静かに誤る」最悪の失敗モード

## BDD受け入れシナリオ

```gherkin
Scenario: 毒値ペアを含む opencode データベースでも analyze が完走する
  Given 同一 assistant メッセージ内に state.time.start = i64::MIN の skill part と
    ts = i64::MAX を与える part を含む合成 opencode データベースがある
  When saniwalens が analyze を実行する
  Then debug ビルドでも panic しない
  And span の開閉と duration_sec が決定論的な値になり
    wrap 由来の負の duration や嘘の idle 継続が出力されない

Scenario: 隣接する毒値ペアの差分は飽和減算の意味論に従う
  Given タイムスタンプ i64::MIN の skill invocation レコードの直後に
    タイムスタンプ i64::MAX のレコードが隣接して並ぶ
  When span_end が idle_gap_ms との比較を評価する
  Then 差分は正側に飽和し idle_gap_ms を超えるため span はそのレコードで閉じる
  And 逆順（i64::MAX の次に i64::MIN）では差分は負側に留まり span は閉じない

Scenario: 正常な opencode セッションの出力は 1 バイトも変わらない
  Given 現実的な範囲のタイムスタンプのみを含む opencode データベースがある
  When 修正前と修正後のバイナリで同じ入力を analyze する
  Then stdout がバイト同一である
```

## 受け入れ基準
- [x] `src/core/span.rs:119`（span_end）と `src/core/metrics.rs:36`
  （duration_sec）の時刻差分が i64 の `saturating_sub` になり、時刻差分経路に
  生の `-` が残らない（usize へのキャストや checked_sub の None 握り潰しは
  しない。型は i64 のまま比較の意味論を保つ）
- [x] opencode / kilocode の ts 読み取りへの暦内制約（claude の
  parse_rfc3339_millis 相当、または 0 以上の epoch ms 範囲）の適用要否が
  実装時に決定され、その決定と根拠が spec またはコードコメントに記録される。
  採択した場合は opencode・kilocode の両方に一貫して適用する
  （kilocode が span 経路に届かないことを理由とした免除をしない）
- [x] opencode 合成 fixture（skill part の `state.time.start` = i64::MIN +
  cost part ts = i64::MAX）で (a) debug で panic しない
  (b) release の span 開閉挙動が決定論的であることを自動テストが pin している
- [x] 正常系 opencode golden が修正前後でバイト同一である
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体（境界値分析・まず失敗するテストから）: span_end / duration_sec に
  i64 の両端（i64::MIN, i64::MAX）とその隣接ペアを直接与えるテスト。現行コード
  では debug で panic することを Red で確認してから飽和化する（Green）。
  両方向（MIN→MAX / MAX→MIN）で飽和方向の意図を pin する
- 単体（同値クラス）: 正常範囲（現実的な epoch ms）の既存テスト群が
  「飽和化で振る舞いが変わらない」ことの証人になる
- 統合: opencode 合成 fixture（skill part の state.time.start = i64::MIN +
  cost part ts = i64::MAX）を parse_session から extract_spans まで通し、
  debug で panic せず span 構造が決定論的であることを確認する
- E2E: 正常 opencode golden の修正前後 diff が空であること。毒 fixture は
  合成のみ（実 DB を持ち込まない — session-data-privacy 規約）

## 実装アプローチ
1. core（必達・最小差分）: `span.rs:119` を
   `record.timestamp_ms.saturating_sub(records[index - 1].timestamp_ms)` に、
   `metrics.rs:36` を `last.saturating_sub(*first)` に置換する
2. 境界（実装時に決定）: opencode の `part_time`（opencode.rs:204-212）と
   DB 読み取り（:269 / :282 / :310）、kilocode の `json_ts`
   （kilocode.rs:184-187）に暦内制約または 0 以上の epoch ms 範囲を適用するか。
   claude は `src/time.rs:83` / :118 で既に適用済みなので、適用するなら
   3 ハーネス統一の形にする。採択しない場合は core の飽和化のみで最小差分
3. 両サイト同時修正: roll_up（span.rs:183-224）は span_end → duration_sec の
   順で同一 window を処理するため、span.rs:119 だけ直すと debug の panic が
   metrics.rs:36 に移るだけ。両方を同時に飽和化する
4. 境界テストで飽和方向の意図を pin し、正常系 golden でバイト同一性を確認

## 見積もり
0.5日。core 2 箇所の機械的な置換と境界テストが実質の作業。
境界強化を採択した場合も、読み取り 2 箇所への同一制約適用で同日収まる範囲。

## 技術的考慮事項
- analyzer version bump 不要: 時刻は fingerprint 経由で既存行の再分析を
  誘発しない。飽和化は出力側の決定論的変化のみ（正常系はバイト同一）
- 信頼境界契約: span.rs:202-205 付近のコメント「the core must still never
  wrap or panic on the values it is handed」— PBI 02 と同じ信頼境界の未完了分
- 3 ハーネス方針: kilocode は SkillInvocation を生成しないため span 経路には
  届かない（反証で確認済み）。ただし時刻読み取り境界は 3 ハーネス統一で
  強化する方針とし、届かないことを免除の根拠にしない
- spec 同期: 境界強化を採択した場合、docs/specs 側へ時刻の暦内制約を記述する
  （spec-sync 規約に従う）

## 実装者向け注記

### 現状コードの確認
- `src/core/span.rs:119` — 生 i64 減算。先行の
  `matches!(record.kind, HumanTurn | SkillInvocation)` の短絡により、現在
  レコードがこの 2 種のときだけ減算が評価されない。Assistant / ToolUse /
  AgentSpawn / Other では必ず評価される
- `src/core/metrics.rs:36` — duration_sec の
  `(last - first) as f64 / 1000.0`。同じ毒が span window の timestamps
  （span.rs:188 で収集）経由で届く
- `src/adapter/opencode.rs:204-212` — `part_time` が part JSON の
  `state.time.start` を `as_i64` 素通し（fallback も DB の生 i64）。
  :269 / :282 / :310 が DB から生 i64 を読む
- 毒値の流れ: 同一 assistant メッセージ内に skill part（state.time.start =
  i64::MIN）と i64::MAX を与える part があると、part list は part_time 順
  （opencode.rs:82）、cost レコードは turn_end = max(part_time)
  （opencode.rs:119-124, :683）で並ぶ → skill invocation（i64::MIN）の直後に
  assistant レコード（i64::MAX）が隣接 → span_end の減算が overflow。
  skill part の分類自体は opencode.rs:563（`part_record_kind`）
- `src/time.rs:83` / :118 — claude 経路は parse_rfc3339_millis が
  年 0..=9999 に制限するため安全（core に届く前に暦内に収まる）
- `src/adapter/kilocode.rs:184-187` — `json_ts` は無制限だが、kilocode が
  生成するレコードは HumanTurn / ToolUse / Assistant のみで SkillInvocation
  を作らないため span 経路には届かない（反証で確認済み）

### 実装手順
1. 失敗するテスト: span_end / duration_sec に毒値ペアを与える単体テストを
   書き、現行コードで debug panic を確認（Red）
2. span.rs:119 と metrics.rs:36 を `saturating_sub` 化（両方同時に —
   片方だけだと panic が移るだけ）
3. 境界強化の要否を決定。採択なら opencode の part_time + DB 読み取り、
   kilocode の json_ts に暦内 / 0 以上の制約を追加し、単体テストを書く
4. 境界テストで飽和方向の意図を pin（両方向の隣接ペア）
5. 正常系 opencode golden のバイト同一性を確認
6. cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
   cargo test

### 落とし穴
- 飽和方向の決定: MIN→MAX ペアの真の差分は巨大な正（saturating で i64::MAX →
  idle 判定になり span が閉じる）。MAX→MIN ペアは巨大な負で、saturating_sub
  は負方向へ正側に飽和しない（→ i64::MIN → idle 判定にならず span が閉じない）。
  前者は「idle とみなす」後者は「span が続き、閉じなければ trailing になる」
  と span 構造の見え方が変わるため、saturating_sub の自然な意味論で各ペアが
  どう記録されるかを確認し、境界テストで意図を pin すること
- 部分修正の罠: span.rs:119 だけ直すと、同じ毒値 window が今度は
  metrics.rs:36 で debug panic する（roll_up は span_end → duration_sec の
  順に同一 window を処理する）。必ず両サイト同時に直す
- 正常系の後退: 飽和化は現実的な epoch ms 範囲では no-op であること。
  既存の duration / idle-gap テスト群が証人になるので、それらを一切変更せず
  green を維持できているかを確認する
