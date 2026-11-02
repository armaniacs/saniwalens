# PBI: headless seam の first-beat を 0 から 3 への 2 段観測に硬化する

## ユーザーストーリー
保守担当者として、heartbeat のテストがスケジューリングの運に依存せず、
「最初の beat は 0 を運び、イベントを足した後の beat は 3 を運ぶ」ことを観測してほしい、
なぜなら現行テストは main thread が 5ms 以上 stall すると count=0 の beat を先に受けて
落ち、逆に `count >= 3` の recv loop にすると「毎回読み直す」退行を検知できなくなるからだ。

## 優先度
- 順位: 6 / 全候補数 10
- RICEスコア: 12.0（Reach=1 / Impact=2 / Confidence=3 / Effort=0.5日）
- 根拠: テストのみの修正だが、spec の「cadence と stop は test で pin」という主張の
  正確性を回復する。⑨（burst 修正）の後に置き、同じテストファイルの変更を直列化する。

## ビジネス価値
- first-beat の競合と straggler の偽陽性が消え、CI の flake が減る。
- `count` を thread 開始時に 1 回だけ読む退行を検知できる（現行も候補の `>=` 案も盲目）。
- 測定方法: 注入遅延 0/6/20/30ms で 200 回緑、cached-count 変異体で赤。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 8本）
- `tests/headless_seam.rs:105-118` は spawn 後に 3 イベントを足し、最初に受信した
  beat の count を `>= 3` で assert する。main thread が 5ms 以上 stall すると
  count=0 の beat を先に受けて落ちる。
- 第 2 の機構: `stragglers <= 1`（:132）は、fetch_add 後に 20ms 遅れると channel に
  溜まった beat を straggler として数え、20/20 で落ちる。
- 候補そのままの `count >= 3` recv loop は first-beat を治すが、`0 -> 3` の遷移を
  見ないため、count を 1 回だけ読む変異体を 15/15 で通してしまう。
- 根本原因: seam の観測点が「サンプル列」しか公開せず、生成側の識別情報（何個目か、
  最初のサンプル時刻）が無いため、テストが生産タイミングを推測して順序仮定を固定する。

## BDD受け入れシナリオ

```gherkin
Scenario: 最初の beat は 0 を運ぶ
  Given まだイベントを 1 件も足していない
  When 最初の beat を受信する
  Then その count は 0 である

Scenario: イベントを足した後の beat は 3 を運ぶ
  Given 最初の beat を受信した
  When 3 イベントを足し、count が 3 の beat まで drain する
  Then deadline 内に count 3 の beat が届く

Scenario: stop 後は終了だけを観測する
  Given stop フラグを立てた
  When receiver を drain する
  Then 停止前に生成済みの beat を捨て、disconnect と join だけを assert する
```

## 受け入れ基準
- [ ] 第 1 相を「最初の beat が 0」->「3 を足す」->「count が 3 の beat まで
  deadline 付き drain」の 2 段観測に置き換える。`>= 3` ではなく `== 3`
- [ ] 第 2 相から `stragglers <= 1` を削除し、disconnect と join だけを assert する。
  「stop 後の送信は高々 1 個」は構造不変条件として heartbeat の doc と spec に書く
- [ ] 2 秒の magic number を `OBSERVE` 定数にまとめ、poll に対して十分大きいことを
  1 行で書く
- [ ] テスト冒頭に「beat はサンプル列であり、個数と時刻はテストの制御下に無い」旨を
  2 行書く
- [ ] panic 時に stop する RAII ガード（`StopOnDrop`）を追加する
- [ ] 実装（`src/cli/heartbeat.rs`）は変更しない
- [ ] spec 更新: `docs/specs/harness.md:189-191` の「pinned by a test」を正確にする
  （cadence は未 pin、stop は終了を pin、高々 1 個は構造不変条件）
- [ ] 変異確認: count を 1 回だけ読む変異体で新テストが赤、注入遅延 0/6/20/30ms で
  200 回緑
- [ ] fmt/clippy/対象テスト/フルスイートが green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存 `the_quiet_headless_delivery_hands_the_advisor_the_seam_renderer` は不変

### 統合テスト
- 注入遅延 0/6/20/30ms の 200 回反復（CPU 飽和下でも）
- 旧テストが同じ条件で落ちることの 1 回確認

### 単体テスト
- 2 段観測（0 -> 3）
- stop 後の disconnect + join
- cached-count 変異体で赤

## 実装アプローチ
- **Outside-In**: 注入遅延で現行の赤を再現 -> 新テストに置き換え -> 変異体で赤 ->
  spec/doc。実装は subagent-driven-development スキルで実施する。

## 見積もり
0.5 SP（E=0.5日。RICE 12.0）

## 技術的考慮事項
- 依存関係: ⑨ -> ①（同じ `tests/headless_seam.rs` と stop 契約）
- 候補の `count >= 3` loop は採用しない。0 -> 3 の 2 段観測にする
- `stragglers <= 1` は観測不能な量なので削除する。弱体化ではなく、観測できる
  「終了」に置き換える
- cadence 自体の pin は別テスト（cadence > poll）が必要。本 PBI の範囲外として
  台帳に残す
- 非機能要件: テストのみ、プライバシー（合成イベントのみ）

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `tests/headless_seam.rs:100-134` の `the_heartbeat_beats_on_the_cadence_and_stops`。
  :105-118 が first-beat、:120-133 が stop/straggler。
- `src/cli/heartbeat.rs:23-33` の spawn doc、:35-58 の loop。
- `docs/specs/harness.md:188-191`。
- probe P1: 6ms stall で 33/40 破れる。P4: 20ms で 20/20。P7: cached-count 変異体は
  現行も `>=` 案も通す。P8: Drop 後も beat 3->21、thread 未終了。

### 実装手順
1. 注入遅延 20ms で現行テストが落ちることを確認（コミットしない）
2. 新テストの 2 段観測を書く
3. cached-count 変異体で赤を確認し、実装を戻す
4. `OBSERVE` 定数と `StopOnDrop` を追加
5. spec と doc を更新
6. 200 回反復 + CPU 飽和で flake を確認

### 落とし穴
- `count >= 3` に戻さない。`0` の次に `3` を要求する
- `stragglers <= 1` を「緩めた」と誤解しない。観測不能な主張を構造不変条件へ移す
- timeout を 10 秒に延ばしても個数依存の競合は消えない
- 実装の cadence/poll を本番比にするとテストが 30 秒かかる。縮小定数のまま

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 注入遅延 0/6/20/30ms で 200 回緑
- [ ] cached-count 変異体で赤
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] harness.md と heartbeat doc 更新済み
