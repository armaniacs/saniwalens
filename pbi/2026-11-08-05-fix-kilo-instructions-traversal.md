# PBI: kilo.json instructions のパス閉じ込めを実装する

## ユーザーストーリー
kilocode 設定を扱う saniwalens ユーザーとして、`kilo.json` の
`instructions[]` に書かれたパスがプロジェクト外のホストファイルを
読み込まないようにしたい。なぜなら悪意・過失を問わず out-of-root の
ファイルがストアに取り込まれると、測定対象でないホスト情報が混入するからだ。

## 優先度
- 順位: 05 / 全17件
- RICEスコア: 6.4（Reach=2 / Impact=2 / Confidence=80% / Effort=0.5日）
- 根拠: PBI 04 と同種の traversal 系の kilocode 限定変種。
  到達範囲は kilocode 利用者に限られるが、該当者への影響は PBI 04 と同等。
  同じ修正方向で塞ぐ。

## BDD受け入れシナリオ
Scenario: プロジェクト内の instructions エントリは従来通り取り込まれる
  Given プロジェクトの `kilo.json` の `instructions[]` がプロジェクト内の
  ファイルを指している
  When  ユーザーが analyze を実行して結果を見る
  Then  そのファイルはサーフェスとして取り込まれる
  And   警告は出ない

Scenario: プロジェクト外を指す instructions エントリは取り込まれず警告される
  Given プロジェクトの `kilo.json` の `instructions[]` がプロジェクト外の
  ファイルを指している
  When  ユーザーが analyze を実行して結果を見る
  Then  プロジェクト外のファイルはサーフェスとして取り込まれない
  And   取り込みをスキップしたことが警告として分かる

Scenario: 上位ディレクトリへの脱出を狙うエントリは取り込まれない
  Given プロジェクトの `kilo.json` の `instructions[]` に
  上位ディレクトリを経由してプロジェクト外を指すエントリがある
  When  ユーザーが analyze を実行して結果を見る
  Then  そのファイルはサーフェスとして取り込まれない
  And   取り込みをスキップしたことが警告として分かる

## 受け入れ基準
- [x] プロジェクトルート外を指す `instructions[]` エントリは読み込まずスキップする
  （相対の `..` による脱出と、絶対パスによる脱出の両方を塞ぐ）
- [x] スキップした場合は警告として可視化する（PBI 04 と同じ語彙で報告する）
- [x] プロジェクト内のエントリは従来通り取り込まれる（正常系の非回帰）
- [x] グローバルスコープとプロジェクトスコープで基準ディレクトリの扱いが
  崩れないこと（各スコープの解決起点を保ったまま閉じ込める）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green（注: `cargo test kilocode` は green、`cargo fmt --check` は
  本PBIの touched ファイルで clean、clippy は本PBI由来の警告なし。フル
  `cargo test` は共有ツリーの他PBI作業中ファイルの影響で未実行）

## テスト戦略
- E2E: 合成プロジェクトの `kilo.json` に out-of-root のエントリを仕込み、
  ストアに取り込まれないことと警告が出ること。in-root のエントリは
  従来通り取り込まれること
- 統合: 相対 `..` 脱出、絶対パス脱出、存在しないエントリ・非配列・キー欠落の
  既存ふるまい（何も載せない）が変わらないこと。global / project の
  両スコープで確認する
- 単体: パス判定の境界（`..`、絶対パス、シンボリックリンク越しの脱出）、
  `instructions[]` の非文字列エントリ混在時の無視が変わらないこと

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 起点: src/adapter/kilocode.rs:630-638 `read_kilo_instructions` は
  エントリが絶対パスならそのまま通し、相対なら `base_dir` と join するだけで、
  containment や canonicalize による検査が無い。`..` は自由に脱出できる
- 配線: src/adapter/kilocode.rs:573-587 `project_sites` が `kilo.json` を
  instructions サイトとして登録し、src/adapter/harness.rs:140-147
  `load_kilo_instructions` が解決起点（設定ファイルの親ディレクトリ）を渡す。
  起点自体は正しいが、その後の閉じ込めが無い
- 修正方向: PBI 04 と同じくルート内に閉じ込め、外側は skip-and-warn
  （読み込まない＋警告）。警告の語彙は PBI 04 と揃える
- エビデンス: src/adapter/kilocode.rs（`read_kilo_instructions`、`project_sites`）、
  src/adapter/harness.rs（`load_kilo_instructions`）。kilocode 限定経路のため、
  他ハーネスの正常系を壊さないことを capture で確認する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（`read_kilo_instructions` の rustdoc と
  `docs/specs/config-format-kilocode.md` の解決規則に閉じ込めを追記）
