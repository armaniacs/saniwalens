# PBI: トレンド/差分ビュー（usage 側のみ）を追加する

## ユーザーストーリー
設定の見直しを続けたい saniwalens ユーザーとして、週次差分で「前回から使用が増えた / 落ちた surface」を検出して提示する機能がほしい、なぜなら使用の変化に気づいて設定を見直すきっかけにできるからだ。

## 優先度
- 順位: 14 / 全28件
- RICEスコア: 4.0（Reach=3 / Impact=2 / Confidence=2 / Effort=3日）
- 根拠: usage（イベント履歴）はストアに歴史的に残るため実現できる。カタログ側の差分（surfaces スナップショット蓄積）は storage.md の契約変更が必要なため対象外とし、この限定により確信度は中。依存: 既存の trend ビューとの重複に注意（実装者向け注記参照）。

## BDD受け入れシナリオ
Scenario: 先週比で使用が増えた surface が提示される
  Given ストアに 2 週間以上の使用履歴があり、ある surface の使用が先週から今週へ増えている
  When  ユーザーが週次の使用差分を表示する
  Then  使用が増えた surface が、差分の量と向き（増えた）とともに提示される
  And   使用が落ちた surface も同じ一覧に現れ、向き（落ちた）が増えたものと区別できる

Scenario: 履歴が 1 週間分しか無い場合は比較不能を明示してフォールバックする
  Given ストアに 1 週間分しか使用履歴が無く、比較できる前の期間が存在しない
  When  ユーザーが週次の使用差分を表示する
  Then  差分の比較ができないことが明示される
  And   通常の usage 表示にフォールバックし、全件落ちたと読めるような誤った差分は出さない

## 受け入れ基準
- [ ] surface 単位の使用差分が週次（既定は直近 7 日とその前の 7 日）で提示され、行ごとに両期間の値・差分・向き（増えた / 落ちた / 同じ）を載せる
- [ ] 差分の対象は usage / events 側のみ。カタログ（surfaces）のスナップショット蓄積は作らない（docs/specs/storage.md の契約「snapshot of now, not an accumulation」を変えない）
- [ ] 比較できる前の期間が無い場合（履歴が 1 週間分のみ等）は、比較不能の旨を明示して通常の usage 表示にフォールバックする。-100% や全件ゼロなど誤読を生む表示を出さない
- [ ] 差分行の並びは差分の大きい順など既存 trend の慣例に揃え、行が無い節は省略する（裸の見出しを出さない）
- [ ] 既存ビューと sql / analyze / optimize の出力は一切変わらない（追加・拡張のみ）
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: 2 週間分の使用履歴がある合成ストアで、増えた / 落ちた surface が向き付きで提示されることをユーザーシナリオとして確認する
- 統合: 2 期間の集計が開始時刻で正しく分かれること（境界・同値・前後関係）、surface と使用の join が両期間で正しく比較されることを確認する
- 単体: 差分の向き判定（増えた / 落ちた / 同じ）、比較不能の判定とフォールバック切替、履歴の境界（1 週間分のみ・空ストア）をテストする

## 見積もり
3日（RICE の Effort と同一根拠）

## 実装者向け注記
- 現状コードの確認（2026-10-31 実施済み）: `trend` ビューが既に存在する（pbi/2026-10-30-02-feat-usage-trend.md、実装済み。src/cli.rs の Command::Trend、105 行目付近）。既存 trend はイベント種別の行・スキル別の増減・セッション/subagent 合計の 2 ウィンドウ差分を出す。本 PBI は重複する実装をしないこと。新規ビューとして切るか既存 trend を拡張するかは、既存 trend の出力と本 PBI の BDD シナリオを突き合わせて決め、新規部分（surface 単位の使用差分、履歴不足時のフォールバック）に絞る
- カタログ履歴（surfaces スナップショット蓄積）は not-to-do（pbi/2026-10-30-00-backlog.md の「やらないこと」節）。docs/specs/storage.md 354 行目付近が「usage は歴史的、catalog は現在」と明文化。本 PBI は usage / events 側の差分のみに限定する
- 歴史的 usage を現在のカタログに join するときの近似に注意: カタログは現在の姿なので、改名・削除された surface の過去使用は現名に寄るか orphan になる。この近似を隠さず、必要なら表示に注記する（Reporting honesty）
- spec 更新: docs/specs/cli.md の Views 表（trend の行）に surface 単位の差分とフォールバックを追記する（cli.md 系の更新が必要になる。動作変更と同じ変更内で）。storage.md は変更しない
- 関連コード: src/cli.rs（Command::Trend、trend 関数 2044 行目付近）、src/store.rs（trend_aggregates / skill_deltas / TwoWindow）
- TDD: e2e（合成ストア）→ 統合（2 期間の surface 別集計）→ 単体（向き判定・フォールバック）の順に赤緑リファクタ
- プライバシー: 合成 fixture のみ。実 transcript はコミットしない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
