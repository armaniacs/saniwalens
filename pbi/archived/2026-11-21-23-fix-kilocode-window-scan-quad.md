# PBI: kilocode の請求窓走査を二分探索で有界化する

## ユーザーストーリー
kilocode のタスクディレクトリを analyze する saniwalens ユーザーとして、
極端に大きい・偏ったタスクファイルが混ざっても解析時間が入力サイズに対して
有界である状態がほしい。なぜなら、現行の請求窓走査は入力サイズに二次で
応答するため、大きなタスク 1 件が analyze 全体を引き延ばし、他のタスクの
結果取得まで遅らせるからだ。

## ビジネス価値
- 優先度: 順位 09 / 13 件中
- 根拠: `src/adapter/kilocode.rs:140-160` の join ループ — 外側ループが
  `starts`（ui_messages.json の全 `api_req_started` 行、:89-102 で収集、
  上限なし）を回り、各 start に対し `assistant_ts.iter().rev().find(...)`
  （:142-147）が毎回末尾から線形走査する。`assistant_ts` は会話ファイルの
  全 assistant ts（:108-123 で収集、上限なし）。インデックス・二分探索なし。
  通常のインターリーブでも比較回数は ≈ S×A/2、start の ts を全 assistant
  より後ろにずらせば S×A。S=A=10^5 で 5×10^9〜10^10 比較
- 到達経路: `kilocode.rs:757` の `analyze` → :811-824 のクロージャで
  `parse_session`、タスクディレクトリごとに課金。fingerprint（:707-718、
  mtime+size）は touch で反復可能なので、再解析のたびに同じコストが載る
- **実害の限定**: 正常な現実タスク（10^3〜10^4 リクエスト）では 10^6〜10^8
  比較で無害。発火には極端なファイルサイズが必要で、影響は**分析遅延のみ・
  正確性不変**
- 改善案: `assistant_ts` は :113 の stable sort に続く in-order push で
  既に ts 昇順に構築されるため、ソート追加は不要。各 start の窓境界探索を
  二分探索（`partition_point`）に置換し、join を O(S×A) → O(S log A)、
  全体を O((S+A) log A) に落とす。出力（請求帰属結果）はバイト同一
  （同一窓の最後の assistant ts を選ぶ現行意味論を維持）

## BDD受け入れシナリオ

```gherkin
Scenario: 帰属結果は 1 バイトも変わらない
  Given 現実的なインターリーブの合成タスクがある
  When 修正前と修正後のバイナリで同じ入力を analyze する
  Then stdout と kilocode の golden がバイト同一である

Scenario: 窓境界の ts と一致する assistant メッセージはその窓に帰属しない
  Given api_req_started 行と同一 ts、および次の行と同一 ts の
    assistant メッセージを含む合成タスクがある
  When parse_session を実行する
  Then 各 cost record の stamp は修正前と同一である
  And 境界 ts の assistant は両隣の窓のどちらにも帰属しない

Scenario: 空の会話スパインでも行自身の ts に fallback する
  Given assistant メッセージを 1 つも含まない会話ファイルと
    usage 行だけの ui_messages がある
  When parse_session を実行する
  Then 各 usage 行は自分自身の ts に cost record を持つ

Scenario: 極端に偏ったウィンドウでも解析が有界時間で完了する
  Given start 行の ts が全 assistant より後ろにずれた
    合成タスク 2 件（サイズ n と 4n）がある
  When それぞれ parse_session の実測時間を測る
  Then 両方とも完了する
  And 時間の比は二次成長の予測（16 倍）から十分に離れている
```

## 受け入れ基準
- [x] `rev().find` の線形走査が二分探索（`partition_point`）に置換され、
  join が O(S log A) になる
- [x] 出力が修正前とバイト同一である（同一窓の最後の assistant ts を選ぶ
  現行意味論を維持、golden diff 空）
- [x] 窓境界の等値条件（`ts == start_ts` / `ts == next_ts` は窓外）が
  テストで pin されている
- [x] エッジケース（`assistant_ts` 空 / 全部 start より前 / 全部後ろ）が
  テストされている
- [x] O(n log n) であることの粗い pin（スケーリング比）がある
- [x] `KILOCODE_ANALYZER_VERSION` を bump しない（挙動不変の純粋リファクタ）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features
  -- -D warnings / cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体: 窓境界の等値条件。assistant ts が start 行と一致する / 次の行と
  一致する各境界で、その assistant が窓に帰属しないことを固定する。
  既存の帰属テスト群（`kilocode.rs:1441` 窓の最後、:1466 行自身への
  fallback、:1479 窓外の孤立 assistant、:1535 マージ順）が等価性の証人
- 単体: エッジ 3 形（`assistant_ts` 空 / 全部 start より前 / 全部後ろ）で
  行自身の ts への fallback を固定する
- 統合: 修正前後のバイナリで同一入力を analyze し、
  `tests/golden/kilocode.txt`（`tests/e2e_golden_harness.rs:190`）との
  diff が空であること。毒入り・巨大 fixture は合成のみ（実 transcript を
  持ち込まない）
- 性能 pin: 大規模合成 fixture のベンチテストは時間 pin になりやすいので、
  「比較回数の上限」や「実測時間の絶対 pin」より**粗い pin**（n と 4n の
  実測時間比が二次成長の 16 倍から十分に離れる、という O(n log n) の pin）
  を推奨。緩いマージンで flake を避ける

## 実装アプローチ
`assistant_ts` が昇順ソート済みであることを利用し、窓探索を
`partition_point` 2 回に置き換える:

```rust
// first index with ts > start_ts（等値は窓外）
let lo = assistant_ts.partition_point(|ts| *ts <= start_ts);
// first index with ts >= next_ts（等値は窓外）
let hi = assistant_ts.partition_point(|ts| *ts < next_ts);
let stamp = if lo < hi { assistant_ts[hi - 1] } else { *start_ts };
```

`rev().find` が返すのは「start_ts より大きく next_ts より小さい最大の
assistant ts」で、`assistant_ts[hi - 1]` と一致する。重複 ts が
`assistant_ts` に含まれても「窓内最大の ts」という値は同一なので、
意味論は保たれる。debug ビルド向けに `assistant_ts` の昇順不変条件を
`debug_assert!` で宣言しておくと、将来の構築順変更に対する保険になる。

## 見積もり
0.5日。置換自体は数行だが、境界の等値条件を既存テストで pin してから
置換する手順と、スケーリング pin の調整が実質の作業。

## 技術的考慮事項
- 純粋なリファクタ（挙動不変）— analyzer version bump 不要
  （`KILOCODE_ANALYZER_VERSION = "6"`、`kilocode.rs:46` は据え置き）。
  出力が変わらない以上、再解析を強制する理由がない
- 大規模合成 fixture のベンチテストは時間 pin になりやすいので
  「比較回数の上限」か「行数に対する実測時間の絶対 pin」より粗い pin
  （O(n log n) であることの pin）を推奨
- 実害は分析遅延のみ・正確性不変。本 PBI は計測の正しさを変えず、
  最悪計算量の裾を切るだけの防御

## 実装者向け注記

### 現状コードの確認
- `src/adapter/kilocode.rs:140-160` の join ループ: 各 start に対し
  :142-147 の `rev().find` が毎回末尾からの線形走査（O(A)）。
  窓条件は `ts > start_ts && ts < next_ts` の**狭義**不等号、
  見つからなければ :147 の `.unwrap_or(*start_ts)` で行自身の ts
- `starts` は :89-102 で収集（全 `api_req_started` 行、上限なし、
  :102 で ts ソート済み）。`assistant_ts` は :108-123 で収集
  （全 assistant ts、上限なし、:113 の stable sort に続く in-order push
  で既に昇順 — 新たなソートは不要）
- 到達経路: :757 `analyze` → :811-824 のクロージャで `parse_session`、
  タスクディレクトリごとに課金。fingerprint は :707-718 `task_fingerprint`
  （mtime+size）で touch により反復可能
- 正常な現実タスク（10^3〜10^4 リクエスト）では 10^6〜10^8 比較で無害

### 実装手順
1. 既存の帰属テスト（:1441 / :1466 / :1479 / :1535）を読み、現行意味論
   （狭義不等号・窓の最後・行自身への fallback）を確認する
2. :142-147 を `partition_point` 2 回の置換に書き換える（上記コード参照）
3. 境界の等値条件テスト（start と同一 ts / next_ts と同一 ts の
   assistant は帰属しない）を追加する
4. エッジ 3 形の fallback テストを追加する
5. スケーリング pin テスト（n と 4n の比）を合成 fixture で追加する
6. `tests/golden/kilocode.txt` との diff が空であることを確認し、
   検証コマンド 3 件を通す

### 落とし穴
- `rev().find` は「start より前（start_ts より大きく next_ts より小さい）
  の最後の assistant ts」を探す — 二分探索の境界（`partition_point` の
  等値条件: `<= start_ts` と `< next_ts`）を**既存テストの帰属結果で
  pin してから**置換すること。`lo` と `hi` の向きや等値の含否を逆にすると、
  黙って別の帰属になる（golden が弾くはずだが、先に pin があれば即特定できる）
- `assistant_ts` が空 / 全部 start より前 / 全部後ろ の 3 エッジでは
  行自身の ts に fallback する（現行 `.unwrap_or(*start_ts)` と同一）。
  `partition_point` はこの 3 形で自然に `lo >= hi` になるが、テストで固定する
- kilocode の既存 golden（`tests/golden/kilocode.txt`、
  `tests/e2e_golden_harness.rs:190` の
  `kilocode_analyze_matches_its_golden`）はバイト同一の証人。
  diff が出たら置換の境界条件の誤りを疑う

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features
  -- -D warnings / cargo test がすべて green
- [x] 帰属結果のバイト同一性を確認済み（`tests/golden/kilocode.txt` の
  diff 空）
- [x] `KILOCODE_ANALYZER_VERSION` を bump していないことを確認済み
- [x] コードレビュー完了（spec 準拠 + コード品質）
