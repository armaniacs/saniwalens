# Changelog

このプロジェクトの主な変更を記録する。記録対象はオリジナル
（[`lambdalisue/cclens`](https://github.com/lambdalisue/cclens) `v0.4.0`）
からの差分のみとする。オリジナル側の変更履歴は含めない。

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従い、
バージョンは [Semantic Versioning](https://semver.org/lang/ja/) に従う。

## [0.5.4] - 2026-11-07

### Added

- 対話ガイド: サブコマンドなし起動（TTY のとき）・`tui`
  サブコマンドで、harness → やりたいこと（doctor / waste / failures）→
  範囲の順に選ぶだけでレポートを表示。実行前後に相当コマンド文を出す。
  実行後は config.toml への保存（default_harness / profile）も選べる。

## [0.5.3] - 2026-11-06

### Added

- 素の Cargo だけで回せる `Makefile` を追加（`make build` / `make test` /
  `make install` ほか、`make check` / `make fmt` / `make clean` /
  `make uninstall`）。Nix や `just` が無い環境でもおなじみの綴りで
  ビルド・テスト・インストールできる。`make install` の既定の置き先は
  `~/.local/bin`（`PREFIX` / `DESTDIR` で変更可）。

### Changed

- 配布クレートの対象外（`justfile` と同等）に `Makefile` を加えた。
  公開物・出力に変更はない。

## [0.5.2] - 2026-11-03

### Added

- レポートビュー 8 つ（usage / inventory / waste / overhead / prompts /
  failures / stuck / doctor）に期間フィルタ（`--since` / `--until` /
  `--last`）を追加。集計が期間に絞られ、引いたウィンドウは freshness 行・
  framing block・JSON の `window` で明示される。フラグ無しの出力は従来と
  バイト同一。`sql` / `analyze` / `optimize` は変更なし（PR #14）。
- `trend` ビュー（2 期間の使用差分）を追加。現在ウィンドウ（既定は直近
  7 日。`--last <N>d|h|w`、`--since`（+ 任意の `--until`）で指定）と直前の
  同じ長さのウィンドウを比較し、種別ごとのイベント集計・スキル別の増減・
  セッション数と subagent の合計を、両ウィンドウの値と差分で行ごとに出す。
  直前ウィンドウにイベントが無い比較は「比較不能」と明示され、-100% と
  いう誤読を生む表示は出ない。既存のビューと `sql` / `analyze` /
  `optimize` の出力は一切変わらない。
- `optimize` の headless 実行に `--timeout <DURATION>` を追加（例:
  `30s`, `2m`, `500ms`）。期限超過で子を kill して reap し、エラーは
  raw log を名指す。既定は無効で、長い実行を勝手に切らない（PR #26）。
- `trend` の JSON に `reason` を追加し、比較不能や実測 0 の理由を
  文字列で返す（PR #15 / #17）。

### Changed

- 起動時解決（設定→フラグ→既定値→ハーネス→ストアパス→旧ストア移行→
  open＋guard）を `Session` モジュールに集約（PR #13）。全コマンドの配線が
  1つの seam になり、不正フラグのエラー表示も従来どおりになった。
- store の ingest を1トランザクションの `IngestBatch` + `Store::ingest` に
  集約し、ストアの open 経路を `Store::open(path, OpenMode)` に統一。
  列オーバーロードの意味の定義を `EVENT_ENCODING` の単一テーブルへ移した。
- `analyze` のハーネス別 dispatch を `Harness::analyze` に集約。入力ルート
  解決と analyzer バージョン鍵を各 adapter へ移し、`cli.rs` は
  dispatch を持たなくなった（4つ目のハーネスは adapter 追加だけで通る）。
- briefing 組立を純粋な `core::findings` へ移設し、上限値（例示数・
  thrash 行数・heavy トークン閾値など）を名前付き定数にした。
- 設定カタログ走査を `catalog_surfaces(harness, roots)` 単一 IF に集約し、
  サイト定義を `Site` のデータ行にした（新 kind は行を足すだけ）。
- `optimize` の配信を `deliver` seam（`DeliveryMode` / `AdvisorLaunch`）に
  切出し、到達不能だった `Option` と `HarnessCopy` の手組みを削除。
- 上記の構造変更は出力が従来と同一: 旧 `821bf1e` バイナリとのE2E出力
  269行がバイト同一、`cargo test` 391件 green。PBI 01-06 は
  `pbi/archived/` にアーカイブ。
- `--format` の宣言を 1 箇所に集約し、ヘルプを実際の受理値（レポートは
  `table | markdown | json`、doctor は `table | json`）に合わせた（PR #24）。
  `doctor --format markdown` は store を開く前に拒否する（PR #17）。
- 期間クエリ用の `events_by_epoch` インデックスを追加した。既存ストアでは
  次回 open 時に作成し、`SCHEMA_VERSION` は変えない（PR #15）。
- 内部構造: CLI を `cli/` 配下へ分割し、windowed flag の宣言を 1 箇所に、
  headless の renderer 選択を harness seam に、`run_headless` を
  `cli/headless.rs` の guard に集約した。出力は不変（PR #15〜#17）。
- ライブラリ API: `saniwalens::store::{SkillUsage, KindRollup, TwoWindow,
  TrendWindowAggregates, SkillDelta, skill_deltas}` は
  `saniwalens::core::trend` へ移動した。互換再輸出はしない（PR #27）。

### Fixed

- `--last` が暦レンジ外を指す場合に丸められる問題と、`trend` の導出
  previous 窓が暦外を指す問題を修正（PR #15）。
- `trend` の point kind で適用外の metric 行が表に出る問題と、`n/a` の
  理由が実際と異なる問題を修正（PR #15）。
- heartbeat が遅延後に burst する問題を修正し、cadence / poll に 0 を
  渡した場合はエラーにした（PR #17 / #21）。

## [0.5.1] - 2026-10-29

### Added

- 設定ファイル（`~/.config/saniwalens/config.toml`）に対応。
  既定ハーネス・既定ストアと名前付きプロファイル（`--profile` で選択）
  を持てる。相対 `db` は config ファイルのあるディレクトリ基準で解決する。

### Changed

- `saniwalens --version` がビルド元の版を名乗る（git タグ由来。ソース
  ビルドの `0.0.0` 表示を解消）。
- `keywords` の `claude` を `coding-agent` に（マルチハーネスの位置づけに合わせる）。

### Fixed

- トークン無しでも CI がグリーンになるよう cachix 手順を条件化。
  リリース側の nix ジョブも同型でスキップする。

## [0.5.0] - 2026-10-28

### Added

- `--harness claude | opencode | kilocode` によるマルチハーネス対応。
  ハーネスごとに独立した SQLite ストアを持ち、分析結果が混ざらない。
- Kilo Code 対応：エディタ `globalStorage` のタスクファイル読み取り、
  トークン行の結合、対話的な `optimize`（`kilo --prompt` セッション起動）。
- opencode 対応：opencode ストアの読み取り、設定カタログ、
  headless / interactive の `optimize` 経路。
- 設定カタログ：commands、agents、skills、`AGENTS.md`、`.kilorules`、
  `kilo.json` の `instructions[]` を計測し、`inventory` / `waste` で
  実際の使用状況と突き合わせる。
- 旧ストアの自動移行：初回起動時に `cclens/` または `agentlens/` を
  `saniwalens/` へ移動し、ハーネス別ファイルも改名する。削除も上書きも
  しない。明示的な `--db` は対象外。
- crates.io への trusted publishing 設定。

### Changed

- ツール名を `saniwalens` に改名。`agentlens` は crates.io で同領域の
  別ツールが取得済みだったため、再改名した。
- 製品文面を coding-agent 横断に汎用化（`AGENTS.md`、README 冒頭と対象範囲、
  クレート文書、flake の説明、アーキテクチャ仕様の一文）。
- GitHub リポジトリを `armaniacs/saniwalens` に、Homebrew tap を
  `armaniacs/homebrew-saniwalens` に。
- `LICENSE` に fork の著作権表示を追加（元の表示は維持）。

### Removed

- Claude Code プラグイン機構を削除（marketplace manifest、
  `plugins/cclens` パック、README の節）。saniwalens はマルチハーネスで、
  Claude Code は主戦場ではない。Claude 専用の用途にはオリジナルの
  `cclens` を案内する。

### Fixed

- ツール失敗の抜粋が JSON エンベロープ全体のまま分類されていた問題を修正。
  実際に運ばれた読めるメッセージで分類するよう unwrapping する。
- work イベントの `started_at` が空のままだった問題を修正。
- 未知のツール名が path-not-found に誤分類されていた問題を修正。
