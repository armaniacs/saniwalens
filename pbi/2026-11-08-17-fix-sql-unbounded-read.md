# PBI: sql の無制限読み取りを方針決定の上で bounded にする

## ユーザーストーリー
saniwalens ユーザーとして、`sql` コマンドで巨大な SELECT を流しても
メモリが際限なく膨らまずに済む動作がほしい。なぜなら現状は stdin と
全行取得が無制限で、cartesian 積や randomblob のようなクエリが
ローカルの自己 DoS（メモリ balloon）になるからだ。ただし「any slice」
契約を壊さずに、安全な形で使いたい。

## 優先度
- 順位: 17 / 全17件
- RICEスコア: 0.8（Reach=1 / Impact=0.5 / Confidence=80% / Effort=0.5日）
- 根拠: ローカルの自己 DoS のみで攻撃者は存在せず、リスクは最も低い。
  素朴な cap は文書化された「any slice」契約と衝突するため、方針決定を
  明示するコストを含めても最下位。

## BDD受け入れシナリオ
Scenario: 通常の小さなクエリは今まで通り全行返る
  Given 小さな結果集合を返す SELECT がある
  When  ユーザーが `saniwalens sql '<SELECT>'` を実行する
  Then  全行がテーブル / JSON で表示される
  And   行数ノート（例: `N row(s)`）が正しい

Scenario: 巨大な SELECT でもメモリが際限なく膨らまない
  Given 何百万行にもなる SELECT（例: cartesian 積や randomblob 展開）がある
  When  ユーザーが `saniwalens sql` でそれを実行する
  Then  決定された方針のいずれかが観測できる:
  ストリーミング出力される、または警告付きで打ち切られ opt-out が効く、
  または文書化された上限エラーで fail-fast する
  And   プロセスが OOM で落ちない

Scenario: 空クエリは従来通りエラーになる
  Given 引数なし・stdin が空の `saniwalens sql` 呼び出しがある
  When  ユーザーが実行する
  Then  `no SQL provided — pass it as an argument or on stdin` 相当の
  エラーで失敗する
  （上限導入が空入力バリデーションを壊していないことの境界確認）

## 受け入れ基準
- [x] 方針を一つに決定し PBI とコードの両方に残す。選択肢は以下の三択:
  （決定: (b) 警告付きcap + opt-out。デフォルト10000行、`--limit 0`で無制限）
  (a) ストリーミング出力（行単位で render し Vec に溜めない）、
  (b) 警告付き cap + opt-out（例: --limit / --no-limit。デフォルト上限あり）、
  (c) document-only（制限を設けず巨大クエリの危険を help / docs に明記）
- [x] (a) または (b) を選んだ場合: 巨大 SELECT の回帰テストがあり、
  メモリ balloon しないこと（行数上限・ストリーミング・警告のいずれか）が
  自動テストで固定される
- [x] (b) を選んだ場合: 素朴な黙示 cap にならないこと。打ち切り時は
  警告を表示し、opt-out（全行取得の手段）が残ること
- [ ] (c) を選んだ場合: `sql` の help と docs に巨大クエリの注意が明記され、
  コード変更なしの判断理由が残ること
- [x] いずれの方針でも「any slice」契約（mod.rs:197-199。任意スライス到達性）
  （`--limit 0` で全行到達可能なため契約維持。JSON配列形状も不変）
  との整合性の判断が明示される（契約を維持する / 契約を改訂する）
- [x] stdin 読み取り（mod.rs:830-836）の上限方針が決まる（SQL テキスト自体の
  （決定: テキストは無制限のまま。増幅するのは取得行のみで、そちらをcap）
  cap の有無と値。無制限のままなら理由を残す）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: 巨大結果クエリ（cartesian / generate_series / randomblob 等の合成）で
  OOM せず方針通りの出力（stream / 警告+cap / 文書化エラー）になること。
  小さなクエリのテーブル・JSON 出力が変化しないこと
- 統合: store の query / query_json が方針通り（stream / 制限 / 現状維持）に
  振る舞うこと。JSON パスとテーブルパスの両方で確認する
  （mod.rs:851-865 の分岐があるため）
- 単体: 空クエリのバリデーション（`query.trim()` 後の empty チェック）が
  残ること。cap 値を設ける場合は境界値（上限ちょうど / 上限+1 / opt-out 時）の
  テスト。stdin 読み取り上限を設ける場合は超過時の振る舞いテスト

## 見積もり
0.5日（RICE の Effort と同一根拠。方針決定 + 実装または文書化 + テスト）

## 実装者向け注記
- 対象1（stdin 無制限）: src/cli/mod.rs の `sql` 関数 830-836行目。
  引数なし時に `std::io::stdin().read_to_string(&mut buf)` で SQL 全文を
  無制限に slurp する。SQL テキスト自体が巨大な場合の上限方針もこの PBI で決める
- 対象2（全行 Vec 化）: src/store.rs の `query`（598-610）と
  `query_json`（616-628）。`query_map` の結果を `collect::<Vec<_>>()` で
  全行集めてから返す。呼び出し側 src/cli/mod.rs:861-865 は
  `store.query(query)?` → `render(...)` / `store.query_json(query)?` →
  オブジェクト化 → `emit_json` であり、render / emit まで全行がメモリ上にある
- 契約の緊張: src/cli/mod.rs:197-199 の `Sql` コマンド doc は read-only SQL で
  「any slice（任意スライス）」に到達できることが契約。素朴な黙示 cap は
  この契約を壊すので、(a) stream（契約維持）/(b) 警告+cap+opt-out（契約維持、
  デフォルト安全）/(c) document-only（契約維持、コード無変更）の三択から選ぶ。
  黙って切り捨てる実装は不可
- 脅威モデル: ローカルの自己 DoS のみ。攻撃者はいない（自分の store に
  自分がクエリを投げるだけ）。優先度を上げないこと。セキュリティ修正として
  扱わず、使い勝手・堅牢性の改善として実装する
- 修正方向の目安:
  (a) rusqlite の行イテレータを render / emit_json に直接流す（テーブル描画側と
  JSON 配列出力側の両対応が必要。JSON は `[` ... `,` ... `]` の incremental
  emit になる点に注意）、
  (b) デフォルト上限（例: N 行）+ 警告 + `--limit <N>` / `--no-limit` 的 opt-out、
  上限超過時は打ち切り行数と opt-out 手段を表示、
  (c) help テキストと docs（sql の節）に巨大クエリ注意を追記しコードは変えない
- 関連コード: src/cli/mod.rs（Sql enum: 200-208、doc 197-199、sql(): 827-867）、
  src/store.rs（query / query_json: 593-628、value_to_string / value_to_json）、
  src/cli/views 等の render / emit_json
- エビデンス: src/cli/mod.rs:197-199（any slice 契約）、830-836（stdin slurp）、
  861-865（collect 後の render）、src/store.rs:598-627（Vec 化）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（cli.mdのsql節に--limitと方針を明記）
