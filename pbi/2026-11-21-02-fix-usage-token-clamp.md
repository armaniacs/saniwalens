# PBI: 非信頼 usage 数値を境界で飽和させる

## 優先度
- 順位: 02 / 全14件
- RICEスコア: 18.0（Reach=6 / Impact=3 / Confidence=100% / Effort=1日）
- 根拠: transcript/config の usage 数値は境界でクランプされず u64 のまま通る。
  毒入り transcript（dotfile 同期・バックアップ復元・ハーネス異常で
  `~/.claude/projects/**` に入り得る）に `u64::MAX` 等を置くと、加算が release
  で静かにラップして ctx_start/peak/growth が嘘の値になり（計測ツールとして
  誤出力）、Debug/テストでは add with overflow でパニック。単一フィールドが
  `i64::MAX` を超えると store.ingest の INSERT が失敗し analyze 全体が中断する

## ユーザーストーリー
usage レポートを見て設定を直す saniwalens ユーザーとして、他のマシンから
同期された破損・改変 transcript が混入しても、トークン計測が嘘を出さず
analyze が最後まで走り切る状態がほしい。なぜなら計測ツール自身が
入力を信頼して数値を歪めたり中断したりするのは、分析の完全性と
ツールへの信頼を根底から崩すからだ。

## 背景
- 入力は非信頼: usage の各フィールドは transcript 内の生の u64 として読まれ、
  境界でクランプされないまま core の加算に流れ込む
- 加算箇所（現行の行番号で確認済み）:
  - `src/adapter/transcript.rs:168-170` — prompt_size の
    input + cache_read + cache_creation の加算
  - `src/adapter/opencode.rs:671` — number("input") + cached("read") + cached("write")
  - `src/adapter/kilocode.rs:143` — usage.tokens_in + usage.cache_reads + usage.cache_writes
  - `src/core/span.rs:202` — out_tokens += out
  - `src/core/metrics.rs:16-21` — ctx_growth の `.sum()`
  - `src/core/usage.rs:23-31` — output_tokens の `.sum()`
  - `src/core/usage.rs:76-79` — share 計算と sub_tokens += share
  - `src/core/usage.rs:168-171` — bundle_models の entry への +=
- store で u64 がそのまま INTEGER に bind される:
  `src/store.rs:858-883`（events への span INSERT）。
  単一フィールドが `i64::MAX` を超えるとここで INSERT が失敗し analyze 全体が中断する
- 改善案:
  1. 上記加算サイトを `saturating_add` に統一する
  2. アダプタ境界（transcript.rs / opencode.rs / kilocode.rs の usage 解析箇所）で
     現実的上限（例: 10^9/フィールド。実装時に確定させる）を超えたフィールドは
     0 扱いにし、スキップ数を数える
  - 境界で正規化すれば core は純粋なまま保てる（core に入力境界の知識を持ち込まない）
- **制約（重要）: 本 PBI は `src/store.rs` に触れてはならない** — 同じ日の PBI 05 が
  store.rs を担当するためファイル排他。境界クランプにより store に渡る値は
  `i64::MAX` を超えないので、INSERT 失敗は store.rs を変更せずに防げる

## BDD受け入れシナリオ

```gherkin
Scenario: 毒入り usage 数値を含む transcript でも analyze が中断しない
  Given input_tokens 等に u64::MAX 近傍の値を持つ assistant 行を含む合成 transcript がある
  When saniwalens が analyze を実行する
  Then analyze はパニックも INSERT 失敗もなく完了する
  And 上限を超えたフィールドは 0 扱いになりスキップ数が記録される

Scenario: 正常な transcript の出力は 1 バイトも変わらない
  Given 現実的な範囲の usage 数値のみを含む transcript がある
  When 修正前と修正後のバイナリで同じ入力を analyze する
  Then stdout がバイト同一である

Scenario: 巨大フィールドの合計でも store への ingest が失敗しない
  Given 複数行それぞれが上限近くの usage を持つ合成 transcript がある
  When analyze が store に ingest するまで走る
  Then すべての INSERT が成功する（境界クランプ後の値は i64::MAX を超えない）
```

## 受け入れ基準
- [x] 3アダプタ（transcript.rs / opencode.rs / kilocode.rs）の usage 解析箇所で、
  フィールド単位の現実的上限を超える値が 0 扱いになり、スキップ数が数えられる
- [x] 上記の全加算サイトが `saturating_add` に統一され、plain `+` による
  u64 加算が usage 経路に残らない
- [x] 境界クランプにより store.ingest へ渡る値が `i64::MAX` を超えない
  （**src/store.rs は本 PBI で変更しない**）
- [x] 正常な transcript に対する全出力が修正前とバイト同一である
- [x] u64::MAX 近傍・i64::MAX 超の境界値を取り込んでも analyze が
  中断しない自動テストがある
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: アダプタ境界のクランプ。上限ちょうど・上限+1・u64::MAX 近傍の各境界で、
  0 扱いとスキップ数のカウントを固定する。既存の通常値テストが等価性の証人になる
- 単体: core の加算（span.rs / metrics.rs / usage.rs）の境界テスト。
  saturating_add 化で既存の通常値の結果が変わらないこと
- 統合: 毒入り（合成）transcript を analyze から store ingest まで通し、
  中断しないことを確認する
- E2E: 正常 transcript での修正前後の出力差分が空であること（既存 golden の diff）。
  毒入り fixture は合成のみ（実 transcript を持ち込まない）

## 見積もり
1日（RICE の Effort と同一根拠）。アダプタ3件 + core の機械的な置換が主で、
上限値の確定と境界テストが実質の作業

## 実装者向け注記
- クランプを入れるのはアダプタの usage 解析箇所のみ。core は既に純粋なので、
  正規化済みの値を受け取るだけでよい（純粋性を壊さないこと）
- transcript.rs は同一 response の行間で out_tokens の増分を
  `saturating_sub` で既に整理している（src/adapter/transcript.rs:172-180）。
  補正対象は 168-170 の prompt_size 加算が中心
- スキップ数の扱い: まずカウントとして保持し、表示するかは後続判断に委ねる
  （本 PBI は「静かに誤る」を「検出可能に落ちる」に変えるのが目的）。
  スキップ数の画面出力を追加する場合は既存ビューのバイト同一性契約と衝突しないよう、
  正常系では 0 で影響が出ない形にする
- 上限値（例: 10^9/フィールド）は実装時に docs/specs 側へ記述を足す
  （spec-sync 規約に従う）
- `src/store.rs` は読んでもよいが変更しない。バインドされる値が
  `i64::MAX` 以下に収まることは境界テストで担保する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` がすべて green
- [x] 正常系出力のバイト同一性を確認済み（修正前後の diff 空）
- [x] `src/store.rs` への変更が無いこと（PBI 05 とのファイル排他）を確認済み
- [x] コードレビュー完了（spec 準拠 + コード品質）
- [x] ドキュメント更新済み（上限値の spec 同期を含む）
