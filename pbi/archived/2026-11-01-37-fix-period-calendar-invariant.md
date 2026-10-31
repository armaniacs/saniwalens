# PBI: 暦の不変条件を入口で明示確立する（日付経路を含める + bounds の空文字 fallback を削除）

## ユーザーストーリー
実装者として、core が作る TimeRange が常に chrono が表現できる instant を名指す
という不変条件が、入口 1 箇所で明示に確立されていてほしい、なぜなら現行は
4-5 site に散在（producer のパッチガード、chrono 合成、consumer の沈黙
fallback、テストの skip）し、失敗形が stale バイナリで実証済みだからだ。

## 優先度
- 順位: 37 / 全候補数 7
- RICEスコア: 12.0（Reach=2 / Impact=2 / Confidence=3 / Effort=1日）
- 根拠: probe で実証済みの失敗形（stale release バイナリが空 bound を本物の
  開示行に描いた）+ 符号依存の構造的弱点（--tz 配線時に現実化）。PBI 29/33 と
  同形・同点。36 と同点だが E が同値のため依存の少ない順（36 が gate の土台）

## ビジネス価値
- Reporting honesty（cli.md:266-273）が名指す anti-pattern の形がコードから
  消える: 暦外 instant は型で表現不能になり、consumer は沈黙 fallback に頼らず
  型の保証に乗れる
- 測定方法: bounds の空文字 fallback が消え、property test が日付込みの無条件
  sweep で非空 bounds を assert すること

## 背景（なぜなぜ分析 2026-11-01 round3、3 立場 x 30 段階、probe 10 点）
- **probe の新事実（前提修正）**: 現行ビルドでは実クロックで CLI から空 bound
  に届く入力は 0 点 ― `usage --since=-262143-01-01`（NaiveDate::MIN）は
  checked_sub_offset（mod.rs:587-593）が守り exit=1、`--since=-262144-01-01`
  はパースエラー、floor+1d / '+' 付き天井は JST シフトが暦内に保つ。
  探索報告の「単窓日付経路 gap」は現行ビルドでは到達不能（実質閉じている）
- **stale release バイナリが失敗形を実証**: target/release/saniwalens（Oct 31
  07:15 ビルド、PBI 29/33 より前）は `trend --since=-262143-01-02` で exit=0 +
  「previous since , until -262143-01-01T23:59:59+09:00」を描いた ― ガード無し
  経路が本物の空 bound を本物の開示行に描く失敗形は現実。検証は必ず再ビルド後
- 根本原因: 暦の不変条件が誰にも所有されず ― producer 2/3 はパッチガード
  （period.rs:61-64, :148-151, :187-193）、日付経路 :70-85 は chrono 合成
  （NaiveDate 範囲 + checked_sub_offset + 未検証 tz_offset_secs の +9 符号）で
  黙って安全、consumer 2 箇所が沈黙 fallback で吸収（bounds :265-275 の
  map_or_else(String::new,…) + store.rs epoch_to_rfc3339_utc :631 + `<> ''`
  :621）、property test は rolling のみ掃き None 素通し（:739, :750-760）
- 符号依存 = 構造的弱点: day_end_epoch :204-206 が変換後の +86400-1 を無検証で
  足すため、負オフセット（--tz 配線時に現実化。spec 宣言済み cli.md:231-236、
  CLI 未配線）なら `--until=+262142-12-31` の end = CAL_MAX+28800 が暦外に
  抜ける。4 桁超の正の年は明示 '+' が要る（符号なし 262142-12-31 はパースエラー）

## BDD受け入れシナリオ

```gherkin
Scenario: 日付由来の current 窓も入口で暦検証される
  When resolve / resolve_pair が日付経路で TimeRange を作る
  Then 作られる TimeRange は暦内である（不変条件が入口 1 箇所で確立される）

Scenario: bounds は空文字 fallback を持たない
  Given 暦外 instant は型で表現不能である
  When bounds が TimeRange を描画する
  Then 空文字 fallback の分岐は存在しない

Scenario: property test は日付を無条件に掃く
  When property test が sweep を走らせる
  Then date 経路込みで非空 bounds を無条件で assert する（None-skip なし）

Scenario: 既存の挙動は変わらない
  Given PBI 29/33 の境界 pin とタイリング不変がある
  When 通常の resolve / resolve_pair / trend を実行する
  Then 既存のエラー文言・出力はバイト同一で、エラーにはならない
```

## 受け入れ基準
- [x] resolve / resolve_pair の日付経路に暦検証を明示確立する
  （既存 error family 維持: --last 由来 invalid-span、日付由来 empty-window
  --since 名指し）
- [x] bounds の空文字 fallback（map_or_else(String::new,…)）を削除する
  （暦外は型で表現不能に。分岐が消えることで型不変条件が consumer に見える）
- [x] property test を日付込みの無条件 sweep に拡張する
  （None-skip 解消: :739 の if let Some を .expect か明示 assert に戻す）
- [x] 順序固定: ガード先・削除後（逆順は沈默誤報が panic に変わるだけ）
- [x] store.rs は直さない（`<> ''` は空-stamp 排除の正当役割に戻る）
- [x] settled pin 回帰禁止: タイリング不変 previous.end == current.start - 1、
  +1 シフト、trend previous 暦外文言、byte-identity、CALENDAR 定数 pin
  （:705-712）、E2E tests/period_errors.rs、--since は必ず clap equals 形
- [x] spec 更新: docs/specs/cli.md Period 節 :186-208 + Reporting honesty
  :266-273（同一変更内）
- [x] 検証は再ビルド後のバイナリで行う（stale release が閉じた帯を再び生かす）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 再ビルド後のバイナリで日付経路の挙動を pin（既存 period_errors.rs に隣接）

### 統合テスト
- resolve / resolve_pair の日付経路の暦検証（PBI 29/33 の edges テストの date 版）

### 単体テスト
- property test の無条件 sweep（date 込み、None-skip なし）
- bounds の fallback 分岐消失を pin

## 実装アプローチ
- **Outside-In**: property test の Red（date 経路が sweep 外という現状を pin）
  から、入口の暦検証 → fallback 削除の順
- 実装は subagent-driven-development スキルで実施する

## 見積もり
1 SP（E=1日。RICE 12.0。PBI 29/33 と同形・同点）

## 技術的考慮事項
- 依存関係: PBI 29/33（完了済み）が前提。PBI 36/38/39/40/41 と独立
- テスタビリティ: resolve/resolve_pair は純粋関数、bounds は純粋
- 非機能要件: プライバシー（probe は一時ディレクトリの空 scratch DB のみ）、

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で probe 実証済み）
- src/core/period.rs: resolve :40-86（rolling :61-64 検証済み、date :70-85
  未検証）、resolve_pair :115-201（derived previous :148-151、date-derived
  previous :187-193）、bounds :265-275（map_or_else 空文字 fallback）、
  day_end_epoch :204-206（+86400-1 無検証）、property test :715-756（rolling
  のみ、None skip :739/:750-760）、CALENDAR 定数 pin :705-712
- src/cli/window.rs:42-58: window_line/window_json が bounds を消費
- src/store.rs: epoch_to_rfc3339_utc :631 + `<> ''` :621（本 PBI では直さない）
- src/cli/mod.rs:587-593: checked_sub_offset（NaiveDate::MIN を守る既存防衛）

### 実装手順
1. Red: property test に date 経路の sweep を足す（現状は sweep 外なので
   空文字 fallback が見える）
2. resolve / resolve_pair の日付経路に in_calendar 検証を明示確立する
3. bounds の空文字 fallback を削除する（ガード後）
4. property test を無条件 sweep に（None-skip 解消）
5. spec 更新（cli.md Period 節 + Reporting honesty）
6. 再ビルド後のバイナリで E2E 確認

### 落とし穴
- probe/review 鮮度: stale release バイナリが閉じた帯を再び生かす。検証は
  必ず再ビルド後のバイナリで
- charset は ―（U+2015）保持、―（U+2014）禁止
- 単窓 8 ビュー（cli/mod.rs:405-576）と cli/trend.rs JSON 分岐には触れない
- 負オフセットは --tz 未配線のため pin は spec 文言 + 定数 pin で（配線は
  別 PBI）

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] bounds の空文字 fallback が消え、property test が日付込み無条件 sweep に
- [x] `just check`（直接実行）と `cargo test` が green（既存 pin 含む）
- [x] コードレビュー完了（task review + 最終 review）
- [x] docs/specs/cli.md 更新済み（Period 節 + Reporting honesty）
