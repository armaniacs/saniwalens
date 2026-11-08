# PBI: Cancelled 誤分類で friction が消える問題を修正する

## ユーザーストーリー
friction レポートを見て設定を直す saniwalens ユーザーとして、ツールエラーの分類が
エラー文の一部一致に左右されず、本物の失敗は friction に残り、本物の
ユーザー中断だけがノイズとして除外される状態がほしい。なぜなら混入した
プロジェクト内容に由来する文言で Cancelled 扱いに倒されると、直すべき
繰り返し失敗が見えなくなり、分析の完全性が崩れるからだ。

## 優先度
- 順位: 01 / 全17件
- RICEスコア: 19.2（Reach=6 / Impact=2 / Confidence=80% / Effort=0.5日）
- 根拠: tainted なプロジェクト内容から到達できる analysis-integrity の穴。
  friction が消える方向の誤分類であり、検出なしに悪化するため最優先。

## BDD受け入れシナリオ
Scenario: 本物のユーザー中断は引き続きノイズとして除外される
  Given 明確なユーザー中断を示すツールエラーがある
  When  ユーザーが friction の一覧（briefing）を見る
  Then  その中断は friction として指摘されない

Scenario: 具体的な失敗原因を含むエラーは Cancelled 風の文言が混ざっても friction に残る
  Given 存在しないファイルを示す具体的なエラーを含み、末尾に中断風の
  文言が付いたツールエラーがある
  When  ユーザーが friction の一覧を見る
  Then  その失敗は path-not-found など具体的な原因の friction として指摘される
  And   Cancelled として除外されない

Scenario: 境界的な短い中断文言だけでは Cancelled に倒れない
  Given 具体的な失敗の証拠が無く、短い中断風の断片だけを含むツールエラーがある
  When  ユーザーが friction の一覧を見る
  Then  その失敗は未分類または具体的な原因側に残る
  And   friction から黙って消えない

## 受け入れ基準
- [x] 具体的な失敗原因の語彙を持つエラーが Cancelled より優先して分類される
  （順序の入れ替えか、Cancelled 側の証拠強化のいずれかで固定する）
- [x] Cancelled 判定は強い証拠（ツール固有の取り消しマーカーや終了意味論など）
  を要求し、単なる部分一致の断片では成立しない
- [x] 攻撃者整形テキスト（具体的な失敗 + 中断風の断片）の回帰テストがある
- [x] genuine な中断（並列キャンセルや権限拒否の確定形）は引き続き Cancelled になる
- [x] 分類の意味論が変わるため必要なら analyzer version の bump を行う
  （未変更 transcript の再抽出に関する規約に従う）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: src/core の分類関数の境界テスト。具体的な原因 + 中断風断片の組み合わせ、
  大文字小文字の揺れ、断片のみ・全文一致のみの各境界を固定する。
  既存の Cancelled 確定形が壊れないことも確認する
- 統合: 分類から briefing 組み立てまでの経路で、Cancelled が除外され、
  具体的な原因が残ることを確認する（件数とラベルで assert）
- E2E: 合成ストアと合成 transcript で waste / friction ビューが変わらないこと
  （genuine 中断は載らない、混入形は具体的な原因で載る）。既存ビューの
  出力差分が無いこと（capture の diff）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- Cancelled の腕は部分一致の語彙で広く拾っている。
  src/core/friction.rs:187-193（`request interrupted by user`、`tool execution aborted`、
  `task cancelled` などの case-insensitive な contains 連鎖）が対象。
  具体的な原因の語彙（path-not-found、edit-precondition など）を含むテキストに
  これらの断片が付くと Cancelled に倒れる
- 除外側の連動: src/core/friction.rs:94-102 の is_actionable が Cancelled を
  非 actionable にし、src/core/findings.rs:161-166 の filter が briefing 前に
  Cancelled セルを落とす。分類が Cancelled に倒れた時点で friction は消える
- 修正方向（いずれか、または併用）:
  (1) Cancelled 側に強い証拠を要求する（完全一致に近いツール取り消しマーカーや
  終了意味論を持つ構造マーカーなど）。(2) 具体的な原因の語彙を先に判定し、
  Cancelled を後段に下げる。どちらを選んでも、genuine な中断の既存テスト
  （opencode 系の確定形など）を壊さないことが条件
- 意味論の変更は抽出結果を変えるため、必要なら CLAUDE_ANALYZER_VERSION の
  bump 要否を判断する（src/core/friction.rs:171-173 のコメントと
  docs/specs/storage.md の規約に従う）
- 関連コード: src/core/friction.rs（classify_error、ErrorCategory::is_actionable）、
  src/core/findings.rs（collect_findings の filter）、src/store.rs（tool_errors の
  書き込みと読み出し）
- エビデンス: docs/specs/events.md（friction 分類の規約）、docs/specs/storage.md
  （analyzer version 規約）、既存の分類テスト（src/core/friction.rs の tests）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み
