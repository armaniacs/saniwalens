# PBI: Never-fires rule ウェッジを実装する

## ユーザーストーリー
path-conditional rule を使う saniwalens ユーザーとして、rule の `paths:` glob に
一致するファイルを扱う tool_use が評価ウィンドウ内に一度も無い rule を
「never-fires rule」として指摘する機能がほしい。なぜなら発火し得たのに
発火していない rule を narrow または delete でき、設定の見直しが測定に
基づいてできるからだ。

## 優先度
- 順位: 04 / 全28件
- RICEスコア: 7.2（Reach=3 / Impact=2 / Confidence=3 / Effort=2.5日）
- 根拠: 仕様で aspirational と明記されたウェッジを計測可能にする。
  CI 系の 3 件（ゲートと可視化）の次。tool_use のファイルターゲット抽出
  （新シグナル）をこの変更内で実装する。

## BDD受け入れシナリオ
Scenario: ウィンドウ内に paths 一致の activity があった rule は never-fires に載らない
  Given `src/**` に一致する rule があり、評価ウィンドウ内のセッションで
  src 配下のファイルを対象に tool_use があった
  When  ユーザーが wedge の一覧（waste ビュー）を見る
  Then  その rule は never-fires rule として指摘されない

Scenario: 一致がゼロの rule はエビデンス付きで載る
  Given `docs/**` に一致する rule があり、評価ウィンドウ内に
  docs 配下のファイルを対象にした tool_use が一度も無い
  When  ユーザーが wedge の一覧を見る
  Then  その rule が never-fires rule として指摘される
  And   指摘には rule の paths glob と、定義の token 数（静的コスト）と
  「ウィンドウ内で一致する対象が一度も無かった」というエビデンスが付く

Scenario: path-conditional でない rule は対象外である
  Given `paths:` を持たない常時読み込みの rule がある
  When  ユーザーが wedge の一覧を見る
  Then  その rule は never-fires rule の対象外である
  （常時読み込みの rule の評価は always-on heavy が担う）

## 受け入れ基準
- [x] path-conditional rule（`paths:` frontmatter 持ち）の glob がカタログに残る
  （surfaces の attrs_json は storage.md に既定済みの列。現状は load_mode の
  判定にしか使われていない）
- [x] tool_use のファイルターゲット（Read / Edit / Write 系の file_path）が
  イベント側に残る。file_edit（Edit / Write / NotebookEdit）は既存なので、
  差分は Read 系の file_path とイベントへの載せ方。v1 スキーマに無い新シグナルとして
  adapter 変更で加える
- [x] core は glob とファイルターゲットの比較を純粋関数で行う（core::glob_match＋never_fires。単体テスト固定）（I/O フリー、
  glob マッチの規約を単体テストで固定）
- [x] ウィンドウ内（--since / --until / --last）で評価し（file_read_targets/window。quiet windowは沈黙）、「unused はウィンドウ内」と
  同じ語彙で報告する
- [x] glob に一致するターゲットが一件も無い path-conditional rule が waste ビューに
  never-fires rule として載り、glob と静的コスト（定義の token 数）を
  エビデンスに持つ。startup_savings 由来の順位規約で path-conditional は
  declutter ランクであることを崩さない
- [x] 一致が一件でもある rule、path-conditional でない rule は載らない（matching・globlessをテストで固定）
- [x] glob が空、または frontmatter が壊れている rule の扱いを決めて固定する（載せない。警告なし）
  （載せないか警告か。誤読を生まない方）
- [x] docs/specs/surfaces.md の「Never-fires rule」節（aspirational→計測済みに更新。events.mdにfile_read、storageは既定済みのため変更なし）（aspirational と明記）を
  計測可能になった実装に合わせて更新し、docs/specs/events.md と
  docs/specs/storage.md にも新シグナルの記述を追加する（spec-sync 規約: 同一変更内）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: 合成ストアと合成 config で waste ビューに never-fires rule が載ること /
  載らないこと。既存 wedge の行と既存ビュー出力が変化しないこと（capture の diff）
- 統合: glob 残し（カタログ側）とファイルターゲット残し（イベント側）が
  同一ストアで結合して評価されること。scope 別（global / project）で
  正しく区別されること
- 単体: glob マッチの境界（区切り、再帰 glob、大文字小文字）、複数 glob のうち
  1 つでも一致したら発火扱い、空 glob、壊れた frontmatter、ウィンドウ境界
  （一致がウィンドウ外にあるときは載る）

## 見積もり
2.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- v1 スキーマに無い新シグナルを 2 つ扱う:
  (1) rule の `paths:` glob の値をカタログに残す。現状
  src/adapter/config.rs の rule_surface / has_paths_frontmatter は load_mode の
  判定にしか使っておらず、Surface ドメインは attrs を持たない。surfaces の
  attrs_json は storage.md に既定済みの列なので schema の migration は不要
  (2) tool_use のファイルターゲット抽出。Read / Edit / Write 系の file_path を
  イベントとして残す。src/adapter/transcript.rs は file_edit（Edit / Write /
  NotebookEdit の target）を既に抽出しているので、差分は Read 系の扱いと
  イベントへの載せ方（既存 work events に加えるか新しい kind にするか）。
  events spec の「New kinds are added additively」の規約に従う
- invalidation: 抽出の意味論が変わるので既存 transcript の再抽出が必要。
  src/adapter/transcript.rs の CLAUDE_ANALYZER_VERSION（meta の analyzer
  version key）を bump する（storage.md の規約: 抽出や分類が
  「未変更の transcript から得られるもの」を変えたら bump）
- glob マッチは純粋関数（src/core/ 配下）。glob の依存を Cargo.toml に足すか
  自前のマッチャにするかは実装時に決める（足す場合は PBI 01 の license
  許可一覧との整合）
- 既存の wedge 分類は変えない。wedge enum（src/core/surface.rs の Unused /
  AlwaysOnHeavy / CostlyRare）に never-fires を追加するのみ。
  waste ビューの既存行が変化しないことを capture で確認する
- spec 更新は必須（spec-sync）: docs/specs/surfaces.md の aspirational の節を
  計測可能になった契約に置き換える。docs/specs/events.md にファイルターゲットの
  シグナルを追加。docs/specs/storage.md にカタログ側の glob 残しを追記
- エビデンス: docs/specs/surfaces.md（wedge 表の「Never-fires rule」行、
  「Requires」配下の aspirational の節、「Usage-measurable vs catalog-only
  surfaces」で rule が catalog-only の節）、docs/specs/events.md（event kinds 表と
  additive 規約）、docs/specs/storage.md（surfaces.attrs_json、analyzer version 規約）
- 関連コード: src/adapter/config.rs（rule_surface / has_paths_frontmatter）、
  src/adapter/transcript.rs（tool_use 分類、file_edit 抽出）、
  src/core/surface.rs（wedge 分類）、src/core/findings.rs、src/cli/mod.rs（waste ビュー）、
  src/store.rs（カタログとイベントの読み書き）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（E2E 3件＋単体。opencode/kilocodeのRead抽出は対象外と明記）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync: surfaces/events更新）
