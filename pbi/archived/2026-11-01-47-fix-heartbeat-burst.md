# PBI: heartbeat の stall 後 burst を捨てて cadence 間隔を保つ

## ユーザーストーリー
利用者として、headless の advisor が長く止まった後に heartbeat が連続で吐かれず、
「30 秒ごとに 1 回」という約束どおりの間隔で動いてほしい、なぜなら stall 復帰時に
500ms ごとの back-to-back burst が起き、同じ秒が並んで「止まって見える」からだ。

## 優先度
- 順位: 5 / 全候補数 10
- RICEスコア: 6.0（Reach=1 / Impact=1 / Confidence=3 / Effort=0.5日）
- 根拠: 修正は 1 行と純関数 + 単体テストに閉じる。①（first-beat）の前提として先行する。
  ⑧（run_headless guard）とは同じ stop 契約だが、別ファイルなので直列でよい。

## ビジネス価値
- stall 復帰後も beat の間隔が cadence 以上になり、liveness 表示が「詰まった」ように
  見えない。
- 測定方法: 縮小定数で stall を注入し、復帰後に 1 beat だけ出て次は cadence 後であること。
  停止後はそれ以上送らないこと。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 7本）
- `src/cli/heartbeat.rs:47-48` は `next += cadence` で次回を進める。stall で
  `elapsed` が `next` を大きく超えると、500ms poll ごとに「1 beat + next を 1 cadence
  進める」を繰り返し、追いつくまで burst になる。
- 実測（縮小定数）: SIGSTOP 6 秒で 7 beat / 0.65 秒。本番定数では 5 分凍結で
  10 beat / 4.5 秒、1 時間で 122 beat / 60 秒、8 時間で約 960 beat / 8 分。
- マシンスリープでは burst しない（Apple の `Instant` は sleep 中に進まない）。
  実トリガはプロセス凍結（SIGSTOP/SIGTSTP/デバッガ）と stderr 閉塞。
- 根本原因: `next` を経過時間から再計算せず、beat 1 回につき 1 cadence 加算で進める。
  かつ「連続 beat の間隔 >= cadence」という不変条件をコードにもテストにも持たない。

## BDD受け入れシナリオ

```gherkin
Scenario: stall 復帰後は 1 beat だけ出る
  Given cadence より長い stall が起きた
  When heartbeat が再開する
  Then 復帰直後に 1 beat を出し、次の beat は cadence 後になる

Scenario: 通常時は cadence 間隔を保つ
  Given stall が無い
  When 2 つの beat を受信する
  Then その到着間隔は cadence 以上である

Scenario: stop 後は送らない
  Given stop フラグが立った
  When receiver を drain する
  Then 追加の beat は送られず、thread が終了する
```

## 受け入れ基準
- [ ] `heartbeat.rs` の `next` 更新を `next = elapsed + cadence` に変え、
  取りこぼした beat は捨てる（catch-up しない）
- [ ] 次回締切の計算を純関数に抽出し、単体テストで直接検証できるようにする
- [ ] 単体テスト 3 本: 連続 beat の間隔が cadence 以上、cadence 超の欠損で 1 beat、
  stop 後の送信 0
- [ ] 既存の退化設定（cadence 1ms < poll 5ms）を cadence > poll の代表値に直すか、
  その意図をコメントで明示する
- [ ] `docs/specs/harness.md` の heartbeat 段落に、cadence / poll / 欠損 skip の
  契約を 1 段落で書く
- [ ] `src/cli/mod.rs` は触らない（行数予算 3,100 に影響させない）
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存 `the_heartbeat_beats_on_the_cadence_and_stops` が不変で green（① の変更前）

### 統合テスト
- 縮小定数で stall を注入し、復帰後の beat 数を数える

### 単体テスト
- 純関数の `next_deadline` が stall 後に `elapsed + cadence` を返す
- 連続 2 beat の間隔 >= cadence
- stop 後に send しない

## 実装アプローチ
- **Outside-In**: stall 注入で burst を再現 -> 純関数 + 1 行修正 -> 単体テスト ->
  spec。実装は subagent-driven-development スキルで実施する。

## 見積もり
0.5 SP（E=0.5日。RICE 6.0）

## 技術的考慮事項
- 依存関係: ⑨ -> ①（`tests/headless_seam.rs` の stop テストを共有）
- `while next <= elapsed` で一気に進める案は、復帰直後に 0.52 秒間隔の beat が出る
  ため弃却。`next = elapsed + cadence` が最小
- cadence の位相（start 基準）は保たない。欠損は捨てる契約を spec に書く
- `cadence = 0` / `poll = 0` の退化引数は公開 API の既知の穴として残す（別 PBI 候補）

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `src/cli/heartbeat.rs:38-45` の spawn loop、:47-48 の `next += cadence`、
  :23-33 の doc。
- `tests/headless_seam.rs:100-134` の `the_heartbeat_beats_on_the_cadence_and_stops`。
- `docs/specs/harness.md:188-191` は「cadence と stop は test で pin」の 1 文だけで、
  30s / 500ms の数値は const と mod.rs:727 のコメントにしかない。

### 実装手順
1. Red: 縮小定数で stall を注入し、burst を再現するテストを書く
2. `next_deadline(elapsed, cadence)` を純関数化し、`next = next_deadline(...)` に
3. 単体テスト 3 本と既存テストの退化設定を整える
4. spec の heartbeat 段落を更新
5. 検証（対象テスト -> fmt/clippy -> フルスイート）

### 落とし穴
- `next += cadence` を残すと burst が再発する。`elapsed + cadence` にする
- stop の二重 check（:39-45）は変えない。停止応答性を悪化させない
- beat の `secs` は `elapsed.as_secs()` のままでよい。同じ秒が並ぶのは burst の症状
- mod.rs の行数予算に触らない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] stall 復帰後に 1 beat だけ出る
- [ ] 連続 beat の間隔 >= cadence が単体テストで pin されている
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] harness.md 更新済み
