# PBI: guide の config 保存競合を修正する

## ユーザーストーリー
guide を使う saniwalens ユーザーとして、並行起動や外部編集があっても
config を失わずに保存したい。なぜなら現状は並行 guide 同士の
clobber やクラッシュウィンドウで config が消失し得るからだ。

## 優先度
- 順位: 15 / 全17件
- RICEスコア: 1.0（Reach=1 / Impact=2 / Confidence=50% / Effort=1日）
- 根拠: config 消失レースの修正。発火は稀だが失うものが大きい。
  到達が稀なため順位は低い。

## BDD受け入れシナリオ
Scenario: 並行する guide の保存が互いの tmp を壊さない
  Given 2 つの guide プロセスが同じ config パスに保存しようとしている
  When  両方がほぼ同時に保存する
  Then  一方の tmp がもう一方の rename を壊さない
  And   config はどちらかの完全な内容になる（半書き・消失しない）

Scenario: クラッシュウィンドウで config が消失しない
  Given guide が保存の最中に kill された
  When  ユーザーが次に config を読む
  Then  旧 config か新 config のどちらか完全な形で存在する
  （tmp 残渣のみで本体が無い状態にならない）

Scenario: 外部編集の上書きは検出されるか仕様として明記される
  Given 保存ダイアログ表示後に外部プロセスが config を更新した
  When  ユーザーが保存を確定する
  Then  上書き前に再読込・比較して警告する、または
  And   last-writer-wins であることが仕様として文書化されている

## 受け入れ基準
- [x] tmp ファイル名が pid-unique（または同等の一意化）になり、
  並行 guide 同士で共有されないこと
- [x] rename 前の `remove_file` をやめ、atomic rename のみで置換すること
  （Unix。Windowsは置換不可のためremove継続を文書化）
  （クラッシュウィンドウで本体が無い状態を作らない）
- [x] 保存確定前に再読込・比較し、外部編集の上書きを検出する、
  または last-writer-wins を仕様として文書化する（どちらかを選んで固定）
  （決定: compare-and-abort。不一致はconflictエラーで保存しない）
- [x] 残渣 tmp の扱い（残す／掃除する）を決めて固定する（残す。自己説明的）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: tmp 名の一意性（pid 含有・衝突しないこと）、remove-then-rename の
  排除（fs 操作の順序 assertion またはモック fs）、再読込・比較分岐
  （一致／不一致／保存中に外部更新）、残渣 tmp の扱い
- 統合: 並行保存の疑似再現（2 スレッド／2 プロセスで同一パスに保存し、
  本体が完全などちらかの内容になること）。クラッシュウィンドウは
  kill 注入または操作順の fault-injection で再現する
- E2E（任意）: tests/guide_flow.rs の保存経路が回帰しないこと

## 見積もり
1日（RICE の Effort と同一根拠）

## 実装者向け注記
- 起点は 3 点:
  (1) src/cli/views/guide.rs:374 の固定 `.toml.tmp` sibling は pid 修飾が無く、
  src/cli/mod.rs:722-729 の private_temp_path（pid＋時刻で一意化）と対照的。
  並行 guide が同一 tmp を共有して clobber する
  (2) 同 :376-377 の remove-then-rename は、remove 後にクラッシュすると
  config 本体が無い状態を残す。rename のみ（上書き rename）で置換する
  (3) 同 :479 のスナップショット取得と :295-340 の render-then-write は、
  間の外部編集を黙って上書きする（lost update）。確定前に再読込・比較する、
  または last-writer-wins を仕様として文書化する
- Windows の rename 語義（既存宛先の上書き可否）に注意。remove 排除後の
  置換が全対応 OS で atomic になる実装を選ぶ
- 関連コード: src/cli/views/guide.rs（persist_config、ask_save、:479 の
  読込側）、src/cli/mod.rs の private_temp_path（一意化の先例）
- spec 更新の要否は実装時に判断する（保存語義を変える場合は guide／config
  spec に last-writer-wins か compare-and-warn かを明記）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（cli.mdの保存語義にcompare-and-abortを明記）
