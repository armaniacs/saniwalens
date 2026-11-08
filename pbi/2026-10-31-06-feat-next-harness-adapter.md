# PBI: 次のハーネス adapter を 1 件追加する

## ユーザーストーリー
claude 以外のエージェント CLI を使う saniwalens ユーザーとして、
codex / gemini-cli / copilot-cli のうち 1 つが `--harness` で選べるようになってほしい。
なぜならその CLI のセッションデータも計測対象に入り、saniwalens が扱える
ハーネスが 3 つから広がるからだ（ccusage は 18 ソース対応、saniwalens は 3）。

## 優先度
- 順位: 06 / 全28件
- RICEスコア: 6.0（Reach=3 / Impact=2 / Confidence=3 / Effort=3日）
- 根拠: feat の同点集団の 2 番目。新ユーザー層を開くが Effort が大きい（3日）。
  どのハーネスにするかは discovery（実データの有無）次第なので、
  着手前に discovery を終えておく。

## BDD受け入れシナリオ
Scenario: `--harness <new>` の analyze がその CLI の実データを読む
  Given その CLI のセッションデータが既定の場所に存在する
  When  ユーザーが `saniwalens --harness <new> analyze` を実行する
  Then  その CLI のセッションが読み込まれ、専用のストアに書き込まれる
  And   その後のビューがそのストアを参照する

Scenario: 既存ハーネスの分析結果が混ざらない
  Given claude / opencode / kilocode の既存ストアが既にある
  When  ユーザーが `saniwalens --harness <new> analyze` を実行する
  Then  既存ストアは読み込まれず、書き換わらない（新ハーネスは独立ストア）
  And   既存の 3 ハーネスのビュー出力は一切変わらない

Scenario: 未知のハーネス指定はエラーになる
  Given saniwalens が扱うハーネスは claude / opencode / kilocode / <new> に
  固定されている
  When  ユーザーが未登録の値（例: 誤字）を --harness に渡す
  Then  コマンドは有効な値の一覧付きでエラーになり、非ゼロで終了する

## 受け入れ基準
- [ ] 選定した 1 ハーネスが `--harness` の選択肢に加わる（global flag、
  既定 claude は不変。既存の起動がバイト同一であること）
- [ ] 新ハーネス専用のストア（同一 state dir の saniwalens-<harness>.db）を持ち、
  既存ストアと共有しない。meta に harness 名と専用の analyzer version key を記録する
- [ ] 他ハーネスのストアを読もうとしたら、期待する --harness（または --db）の
  ヒント付きで拒否される（既存契約の踏襲）
- [ ] adapter モジュールが入力の発見、fingerprint スキップ、抽出、
  ストア引き渡しを担い、Harness::analyze が唯一の dispatch である
  （cli/mod.rs は dispatch arm を増やさない）
- [ ] session-format の新 spec（docs/specs/session-format-<harness>.md）を
  docs/specs/ に新設し、docs/specs/harness.md に「One store per harness」の表、
  入力 root、v1 capability の節を「Forward compatibility」の契約どおり追加する
  （spec-sync 規約: 同一変更内）
- [ ] 計測できないシグナル（spawn / prompt / denial など）はゼロのままとし、
  誤った値で埋めない（report the gap, never substitute）
- [ ] 未登録の --harness 値は有効な一覧付きでエラーになる
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- E2E: 合成 fixture（その CLI の実形式のセッションデータを合成）で
  `--harness <new> analyze` が走り、専用ストアに書き込まれること。
  既存 3 ハーネスの出力が変わらないこと（capture の diff）
- 統合: 専用ストアの schema が既存と同一であること（storage.md どおり）。
  クロスハーネス読み取りの拒否。meta の harness 名と version key
- 単体: adapter の抽出（セッション境界、プロジェクトの推定、タイムスタンプ、
  トークン）、fingerprint スキップ、成長中ファイルの torn read、
  未知のハーネス値

## 見積もり
3日（RICE の Effort と同一根拠）

## 実装者向け注記
- 候補: codex / gemini-cli / copilot-cli。選定は discovery で実データ
  （セッションファイルの有無、形式の安定性）から決める。
  実データが無いハーネスは合成 fixture だけで実装すると嘘の計測になる恐れが
  あるので、有データのものを優先する。この PBI は 1 件のみ（残りは別 PBI）
- 契約（docs/specs/harness.md の「Forward compatibility」）: 新ハーネスは
  (1) Harness variant、(2) 既定入力 root とストア名、(3) format spec、
  (4) adapter module（Harness::analyze の背後、入力 route / fingerprinting /
  analyzer version key 込み）の追加で済み、shell（cli/mod.rs）は dispatch を増やさない。
  「新ハーネスは variant と module の追加で済む、shell は変更しない」を守る
- 既存 3 ハーネスの実例: src/adapter/opencode.rs（SQLite 読み）、
  src/adapter/kilocode.rs（ディレクトリ内 2 JSON）、src/adapter/transcript.rs（JSONL）。
  入力形式に応じて最も近い実例を踏襲する
- 独立ストア: saniwalens-<harness>.db（同一 state dir）。schema は既存と同一
  （storage.md）。クロスハーネスの拒否は既存の guard_harness と meta の
  harness キーの踏襲
- analyzer version key はそのハーネス専用に新設（CLAUDE / OPENCODE /
  KILOCALODE と同列）。bump 規約は storage.md どおり
- spec 更新は必須（spec-sync）: docs/specs/session-format-<harness>.md を新設、
  docs/specs/harness.md にストア表・入力 root・v1 capability の節を追記。
  docs/specs/storage.md は schema が同一なら変更なし
- プライバシー: fixture は合成のみ。実 transcript / 実 config はコミットしない
  （.claude/rules/session-data-privacy.md）
- エビデンス: docs/specs/harness.md（One store per harness / --harness /
  v1 capability / analyze seam / Forward compatibility の各節）、
  docs/specs/storage.md（schema、incremental ingest、analyzer version）、
  docs/specs/architecture.md
- 関連コード: src/adapter/harness.rs（Harness enum / parse / analyze /
  catalog_surfaces）、src/adapter/opencode.rs / kilocode.rs / transcript.rs（実例）、
  src/cli/mod.rs（--harness flag）、src/session.rs（Session::open と guard）

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み

## Discovery 結果（2026-11-08）: データ無しのため着手見送り

実装前の discovery（PBI 本文の指示どおり）を 2026-11-08 の作業機で実施した。
3 候補いずれにも、そのまま計測できる実セッションデータが無かった:

- codex: `~/.codex` が存在しない（セッションファイル無し）。
- gemini-cli: `~/.gemini` には antigravity-cli の cache/summaries と設定のみ。
  gemini-cli 本体のセッション transcript（JSONL 等）は無い。
- copilot-cli: `~/.copilot/logs/process-*.log` はあるが daemon の process log
  名であり、構造化セッションではない。内容の private 性を守るため中身の
  目視確認は行わず、セッション転送の根拠が無いものとして扱う。

PBI 本文の「実データが無いハーネスは合成 fixture だけで実装すると嘘の計測に
なる恐れがある」に従い、推測での adapter 実装は行わない。
着手条件: いずれかの CLI の実セッションデータがある環境での再 discovery。
seam 側の準備は不要（Harness enum＋Forward compatibility 契約は現状で足りる）。
