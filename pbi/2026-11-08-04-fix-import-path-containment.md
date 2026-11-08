# PBI: @-import のパス閉じ込めを実装する

## ユーザーストーリー
信頼できないリポジトリを扱う saniwalens ユーザーとして、クローンした
リポジトリの `CLAUDE.md` / `AGENTS.md` に書かれた `@`-import が
プロジェクト外のホストファイルを読み込まないようにしたい。なぜなら
悪意・過失を問わず out-of-root のファイルがストアに取り込まれる
（トークン数とパスのみ。内容自体は取り込まない）と、測定対象でない
ホスト情報が混入するからだ。

## 優先度
- 順位: 04 / 全17件
- RICEスコア: 6.4（Reach=4 / Impact=1 / Confidence=80% / Effort=0.5日）
- 根拠: 全ハーネス共通の path traversal 系。影響はメタデータ
  （トークン数とパスのみ、内容は含まない）に限定されるため Impact は小さいが、
  攻撃面として先に塞ぐ。

## BDD受け入れシナリオ
Scenario: プロジェクト外を指す @-import は取り込まれず警告される
  Given プロジェクトの `CLAUDE.md` がプロジェクト外のファイルを
  `@` で参照している
  When  ユーザーが analyze を実行して結果を見る
  Then  プロジェクト外のファイルはサーフェスとして取り込まれない
  And   取り込みをスキップしたことが警告として分かる

Scenario: プロジェクト内の @-import は従来通り取り込まれる
  Given プロジェクトの `CLAUDE.md` がプロジェクト内のファイルを
  `@` で参照している
  When  ユーザーが analyze を実行して結果を見る
  Then  参照先のファイルはサーフェスとして取り込まれる
  And   警告は出ない

Scenario: 絶対パスでプロジェクト外を指す @-import は取り込まれない
  Given プロジェクトの `CLAUDE.md` が絶対パスでプロジェクト外のファイルを
  `@` で参照している
  When  ユーザーが analyze を実行して結果を見る
  Then  そのファイルはサーフェスとして取り込まれない
  And   取り込みをスキップしたことが警告として分かる

## 受け入れ基準
- [x] プロジェクトルート外を指す `@`-import は読み込まずスキップする
  （相対の `..` による脱出と、絶対パスによる脱出の両方を塞ぐ）
- [x] スキップした場合は警告として可視化する（見落とされない方）
- [x] プロジェクト内の `@`-import は従来通り取り込まれる（正常系の非回帰）
- [x] `~/` 形式の参照の扱いを決めて固定する（root外としてskip+warnに統一。carve-outなし）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: 合成プロジェクトの `CLAUDE.md` / `AGENTS.md` に out-of-root の
  `@`-import を仕込み、ストアに取り込まれないことと警告が出ること。
  in-root の import は従来通り取り込まれること
- 統合: 相対 `..` 脱出、絶対パス脱出、`~/` の各入力に対する取り込み可否が
  スコープ別（global / project）で正しく区別されること
- 単体: パス判定の境界（`..`、絶対パス、`~/`、シンボリックリンク越しの
  脱出、存在しないパス）、循環 import と多重到達の既存ふるまいが変わらないこと

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 起点: src/adapter/config.rs:367-372 `resolve_import` は `@`-import の
  raw パスを `base_dir` と単に join するだけで、containment の検査が無い。
  `~/` のみ special-case され、それ以外は `base_dir.join(raw)` に落ちる
- std の join 意味論により、raw が絶対パスの場合は `base_dir` が破棄され、
  そのまま絶対パスが通る（相対 `..` も join 後の検査が無いため自由に脱出できる）
- 呼び出し側 :416-427 は canonicalize 可能なターゲットを無条件に読み込み、
  ストアに入れる。取り込まれるのはトークン数とパスのみで内容は含まないが、
  out-of-root のホスト情報が混入する点は変わらない
- 修正方向: canonicalize した上でプロジェクトルート内に閉じ込め、
  外側は skip-and-warn（読み込まない＋警告）。`~/` の扱いは実装時に決める
- エビデンス: src/adapter/config.rs（`resolve_import`、`read_claude_md_surfaces` の
  import 解決ループ）。全ハーネス共通経路のため、修正は他ハーネスの
  正常系を壊さないことを capture で確認する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（config-format.mdのimport規則）
