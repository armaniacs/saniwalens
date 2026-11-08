# PBI: guide entry のフラグ値取り込みで --help が飲まれる問題を直す

## ユーザーストーリー
CLI のヘルプを見たい saniwalens ユーザーとして、`saniwalens --harness --help`
のような入力でガイド画面に連れて行かれず、期待通りヘルプが表示されて
ほしい。なぜならヘルプ要求が別画面にすり替わるのは確実な UX の罠だからだ。

## 優先度
- 順位: 11 / 全17件
- RICEスコア: 4.0（Reach=2 / Impact=0.5 / Confidence=100% / Effort=0.25日）
- 根拠: 範囲は狭いが再現が確実な UX トラップ。修正は小さい。

## BDD受け入れシナリオ
Scenario: --harness の次にフラグが来たらclapに渡される（happy path）
  Given ユーザーが `saniwalens --harness --help` と入力した
  When  argv 分類（guide_entry）が実行される
  Then  呼び出しは Normal として clap に渡される
  And   ガイド画面には入らず、clapのmissing-valueエラーになる（exit 2）

Scenario: --profile の次にフラグが来ても同様に Normal になる
  Given ユーザーが `saniwalens --profile --version` と入力した
  When  argv 分類が実行される
  Then  呼び出しは Normal として clap に渡される
  And   ガイド画面には入らない（clapがmissing-valueエラーを出す）

Scenario: 正常なプリセット値は引き続きガイドに渡る（boundary）
  Given ユーザーが `saniwalens --harness opencode` と入力した
  When  argv 分類が実行される
  Then  呼び出しは Guide として扱われる
  And   harness プリセットに `opencode` が残る

## 受け入れ基準
- [x] `--harness` / `--profile` の直後が `-` で始まる値のとき、guide ではなく
  Normal にフォールバックすること
- [x] `--flag=value` 形式と `tui` 明示エントリの既存挙動が変化しないこと
- [x] 値欠落（末尾の `--harness` のみ）の既存 Normal フォールバックが維持されること
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: guide_entry の argv 分類テーブルを固定する。`--harness --help`、
  `--profile --version`、`--harness= --help` 系は Normal、`--harness opencode`、
  `--harness=opencode`、`tui` 単独は Guide。`--harness -x` のような
  ダッシュ付き値は Normal
- E2E: 実バイナリで `--harness --help` がヘルプを出し、exit 0 でガイドに
  入らないこと。`--harness opencode`（サブコマンド無し）がガイドに入ること

## 見積もり
0.25日（RICE の Effort と同一根拠）

## 実装者向け注記
- 原因は src/cli/views/guide.rs:44-53: `--harness` / `--profile` の直後に
  `args.next()` を無条件で値として消費するため、`--help` のような
  フラグに見えるトークンまで値として飲み込み、help 要求がガイド経路に
  振り分けられる。同ファイル :54-57 の `-` フラグ→Normal 規約は、値位置の
  トークンには適用されていない
- 修正方向: 値位置のトークンに dash-check を足し、フラグに見える値
  （`-` で始まる）の場合は Normal にフォールバックする。`--flag=value` 形式
  （:34-43、空値は既に Normal）は対象外でよい
- 既存の純粋 argv 分類（guide_entry）の形を保ち、I/O を持ち込まない。
  `--harness -` のような境界値の扱い（値かフラグか）をテストで固定する
- エビデンス: docs/specs/cli.md（guide entry と argv 分類の節）
- 関連コード: src/cli/views/guide.rs（guide_entry のみ）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（契約不変のためspec更新なし。BDD文面を実契約に合わせた）
