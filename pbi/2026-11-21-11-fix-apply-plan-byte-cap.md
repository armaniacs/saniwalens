# PBI: --apply の plan 生成と実行ログ読みに上限を設ける

## ユーザーストーリー
`--apply` で最適化 plan を対話セッションに引き継ぐ saniwalens ユーザーとして、
アドバイザが多弁でも plan 生成と生ログ読み込みがメモリ・ディスクの上限に収まって
ほしい。なぜなら上限のない生ログ `read_to_string` と `Text` 行の無制限 join は、
`--timeout 0` のもとで GB 級ログを丸ごとメモリに載せて OOM で handoff 全体を
死なせ、plan ファイルも state ディレクトリに際限なく蓄積するからだ。

## 優先度
- 順位: 11 / 全14件
- RICEスコア: 6.0（Reach=3 / Impact=1 / Confidence=100% / Effort=0.5日）
- 根拠: plan 生成経路にサイズ上限がなく、リソース安全性の穴。正常系は既定 60s
  タイムアウトで抑えられるため顕在化は `--timeout 0`（明示的な無期限オプトアウト）
  や多弁なアドバイザ寄りだが、1回でプロセスが死ぬ性質の問題であり、
  briefing 側の既存の正例に合わせて閉じる。詳細は背景に。

## 背景
`--apply` の handoff は headless アドバイザの生ログを材料に plan ファイルを
作る。この経路にサイズ上限が一切ない:

- `capture_headless_log` が生ログを丸ごと `read_to_string` する
  （src/cli/handoff.rs:264-268 付近）。成功時はログを消すため、中身を全て
  メモリに載せるしかない
- `complete_handoff` が全行を `Vec<RunLine>` に materialize した後
  （src/cli/handoff.rs:195-203）、`extract_plan_text` が `Text` 行を無制限に
  join して plan ファイルに書く（src/cli/handoff.rs:204-211 付近、
  src/adapter/harness.rs:47-56）
- plan ファイルは state ディレクトリに無上限で蓄積する（`save_plan`、
  src/cli/handoff.rs:145-150）。cleanup の掃除は世代数のみでサイズ不問
  （src/cli/cleanup.rs:53-56 付近の `select_victims`）

対照的に briefing 側は `BRIEFING_MAX_BYTES` で bounded だ
（src/core/optimize.rs:481-512 付近）。plan 側には対応する上限がなく、
`--timeout 0`（明示的な無期限オプトアウト、src/cli/headless.rs:220-222、
`parse_timeout` 全体は :200-231 付近）や多弁なアドバイザで GB 級ログ → OOM、
plan ファイルも無上限で蓄積する。正常系は既定 60s タイムアウト
（src/cli/headless.rs:194）で抑えられる。

改善案: plan 抽出に上限（briefing と同値の `PLAN_MAX_BYTES`、超過で
先頭 N + マーカー保存）を設け、`capture_headless_log` の読み込みも同上限で
打ち切る。「plan が上限を超えた」ことを stderr に明示すれば handoff の
意味も壊さない。

## BDD受け入れシナリオ

```gherkin
Scenario: 上限内の plan は従来どおりバイト同一で保存される
  Given Text 行の合計が PLAN_MAX_BYTES 以内の headless 実行ログがある
  When --apply の handoff が plan を抽出して保存する
  Then plan ファイルの内容は既存実装とバイト同一である
  And stderr に打ち切りの表示は出ない

Scenario: 上限超過の plan は先頭 N + マーカーで打ち切られる
  Given Text 行の合計が PLAN_MAX_BYTES を超える合成入力の実行ログがある
  When --apply の handoff が plan を抽出して保存する
  Then plan ファイルは先頭 N + 末尾マーカーで打ち切られている
  And stderr に「plan が上限を超えた」ことが明示される
  And handoff 自体（dialog 起動まで）は続行する

Scenario: 生ログの読み込みも同上限で打ち切られる
  Given --timeout 0 で動く多弁なアドバイザが PLAN_MAX_BYTES を超える生ログを残した
  When capture_headless_log がそのログを読む
  Then 読み込みは同上限で打ち切られ、handoff は最後まで完遂する
  And 打ち切られたことが plan 生成側の表示に引き継がれる
```

## 受け入れ基準
- [x] plan 抽出に上限（`PLAN_MAX_BYTES`、briefing の `BRIEFING_MAX_BYTES` と
  同値）があり、超過 plan は先頭 N + 末尾マーカーで保存される
- [x] `capture_headless_log` の生ログ読み込みも同上限で打ち切られる
- [x] plan が打ち切られたことは stderr に明示される（黙って切らない）
- [x] 上限内の入力では既存とバイト同一の出力で、既存 handoff テストが
  無変更で green
- [x] `--timeout 0`（無期限オプトアウト）でも上限制御が機能する
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: 上限ヘルパー（truncate / cap 関数）の境界テスト。上限ちょうど・
  1バイト超過・UTF-8 文字境界またぎ、マーカー込みで上限に収まることを固定。
  `extract_plan_text` 自体は既存契約（`Text` のみ stream 順 join）を維持する
- 統合: `complete_handoff` の double テストで上限超過の合成ログ → plan
  ファイルが先頭 N + マーカー、stderr 明示、dialog 起動まで完遂すること。
  既存の `complete_handoff` テスト（保存・空 plan 拒否・dialog 失敗時 keep）
  が無変更で green なことが回帰の証人
- E2E: `capture_headless_log` の合成 advisor（実 spawn + 合成 JSON）で
  超過ログの読み込み打ち切りを確認する。正常系のバイト同一性は既存
  handoff テスト全体が証人

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 触る箇所: src/cli/handoff.rs（`complete_handoff` :195-211 付近 /
  `capture_headless_log` :241-270 付近、読み込みは :264-268 付近 /
  `save_plan` :145-150）、src/adapter/harness.rs（`extract_plan_text`
  :47-56）。対比の正例: src/core/optimize.rs:495 の `BRIEFING_MAX_BYTES` と
  :506-518 の `truncate_briefing`（上限値・末尾マーカー・文字境界処理の形を
  踏襲する）
- 上限の位置: 抽出後の plan 文字列に適用する（先頭 N + マーカー保存）。同時に
  `capture_headless_log` の `read_to_string` を同上限で打ち切る（チャンク読み
  + 上限到達で打ち切り）。片側だけではメモリ・ディスクのどちらかが塞がり残る
- 打ち切り表示: stderr に「plan が上限を超えた」旨を明示する。handoff の目的
  （advisor の plan をそのまま渡す）は打ち切りで損なわれるが、明示があれば
  利用者が事実を知った上で続行できる。`BRIEFING_TRUNCATION_MARKER` の形
  （末尾マーカーで切れたことが分かる）に合わせる
- `--timeout 0` は明示的な無期限オプトアウト（src/cli/headless.rs:220-222、
  `resolve_timeout` が警告を出す）。既定 60s（:194）で正常系は抑えられるが、
  本 PBI はタイムアウトに依存せずサイズで閉じるのが狙い
- cleanup は世代数のみでサイズ不問（src/cli/cleanup.rs:53-56 付近の
  `select_victims`）。plan ファイル1件のサイズ上限が state ディレクトリ蓄積に
  対する唯一の防御になる
- IMPORTANT（着手順）: 本 PBI（11）は src/cli/handoff.rs を触る。同日の
  PBI 12（optimize-apply-prologue、プロローグ共通化）も handoff.rs を触るため、
  必ず 11 を先に着手し完了後に 12 を開始すること。12 側もこの制約
  （handoff.rs に置かれる plan 上限の定数・関数配置）を知っている前提なので、
  プロローグ共通化の際に上限まわりを壊さないこと
- 検証: cargo fmt --check / cargo clippy --all-targets --all-features --
  -D warnings / cargo test

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] 上限ちょうど・超過・以内の境界がテストで固定されている
- [x] 既存 handoff テストが無変更で green（正常系のバイト同一性）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・
  干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
