# Changelog

このプロジェクトの主な変更を記録する。記録対象はオリジナル
（[`lambdalisue/cclens`](https://github.com/lambdalisue/cclens) `v0.4.0`）
からの差分のみとする。オリジナル側の変更履歴は含めない。

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従い、
バージョンは [Semantic Versioning](https://semver.org/lang/ja/) に従う。
各節はタグ間差分単位で記録する（ブランチ差分単位ではない）。

## [0.6.8] - 2026-11-22

### Fixed

- `analyze` が `~/.claude/projects` 不在の環境（新規ユーザー・fresh CI
  ランナー・nix sandbox）で `read projects dir` エラーになり、空セッション
  として成功しない問題を修正。不在を零セッションとして扱い、空ストアを
  作って成功する。v0.6.6 / v0.6.7 のリリースビルドがこの問題で失敗して
  いた — リリースビルドのテストが実ホストの `~/.claude` を実読みして
  いた非決定的依存（v0.6.6: `tests/advisor_language.rs` の seed、v0.6.7:
  `tests/apply_handoff.rs` の seed）を、合成 `CLAUDE_CONFIG_DIR` の
  hermetic 構成に変更済み。v0.6.7 は crates.io に publish されていない
  （Homebrew tap は v0.6.5 のまま）。

## [0.6.7] - 2026-11-22

### Fixed

- `tests/advisor_language.rs` の seed analyze が実ホストの `~/.claude/projects`
  を実読みしており、nix sandbox のように projects dir が不在な環境で
  リリースビルドが失敗する問題を修正。seed を空 fixture `projects` dir +
  `CLAUDE_CONFIG_DIR` の hermetic 構成に変更する。実コード（src/）への
  変更はなく、v0.6.6 のリリースビルドが crates.io / Homebrew tap まで
  到達しなかったことの補填。テストから発見した新規ユーザー環境での
  `analyze` 失敗（projects dir 不在時に Err）は台帳に PBI 候補として
  記録（本バージョンでは未修正）。

## [0.6.6] - 2026-11-22

### Added

- 性質ベーステスト（proptest）と熱経路ベンチ（criterion）を導入。
  analyze 100k 行・glob 照合・kilocode 窓帰属の 3 ベンチを常設する。

### Changed

- `optimize --frozen`（ガイド内の frozen 実行も同様）を本当に読み取り
  専用にした。ストアの作成・chmod・移行は行わず、不在ならエラー、既存
  なら経過時刻を表示する。
- `--timeout` を headless の全経路（`--verbose` 込み）に適用した。対話
  経路は期限を持たない旨を明示警告する。
- 破損・毒入り transcript への耐性を強化。不正 UTF-8 行はスキップして
  警告し、型不正の usage は 0 扱いで計数（行は消えない）、usage 数値は
  フィールドごとに 10^9 で飽和、時刻差分も飽和する。Claude 経路の
  analyzer version が 11 に上がるため、既存ストアは次回 open 時に
  再分析される。
- `waste` の never-fires 判定に照合予算を導入し、read-target 照会に
  上限（5,000）を設けた。予算が尽きた rule は名指しして警告する。
- 設計・実装整理（出力不変）。共有 row 型を core へ移管、ハーネス静的
  事実を `HarnessFacts` に集約、`session` の表示/解決を分離、`optimize`
  と `--apply` のプロローグを統一、範囲クエリの組立を集約。
- 引き続きの DRY/KISS 整理（出力不変）。`AgentCost` を core の単一定義
  に統一（store の briefing クエリが core の型を直接返す）、private
  ディレクトリ系プリミティブを store の 1 箇所に統一（警告の
  transport は各層が担う）、5,002 行だった `store.rs` を
  `store/{schema,migrate,briefing,permission,tests}` へ分割（`pub use`
  で `crate::store::` の経路は全て維持）。

### Fixed

- 端末出力の sanitize を完全化。生 C1（U+009B など 8-bit CSI）、
  bidi/ゼロ幅（U+2066-2069 含む）、CR を除去し、doctor の prose に
  transcript 由来の agent 名が素通ししていた経路を塞いだ。
- briefing の `by tool:` 行に transcript の tool 名が改行つきで流れ、
  指示注入できた経路を単行化で塞いだ。
- ガイドが手編集 config（行末コメント付きヘッダなど）に対してテーブル
  重複の不正 TOML を書き、次回起動が失敗する問題を修正。
- `--apply` の plan がログ打ち切りで尻切れでも marker 無しで保存される
  問題を修正。
- kilocode の請求窓帰属を二分探索化し、偏ったタスク（10 万件規模）で
  analyze が二次時間かかる問題を修正。
- private ディレクトリやストアが他人所有・group/world 書き込み可でも
  黙って使い続ける問題を修正（警告を出す）。
- ストア移行の version guard がトランザクション外で、異版並行時に
  素通りしうる問題を修正（guard を同一 tx 内へ、busy_timeout 5 秒）。
- ストア移行の収束対象が `subagent_runs` を列挙から漏らし、宣言への
  列追加時に既存ストアで INSERT が全滅しうる潜在問題を、未分類テーブル
  を移行時に失敗させるガードで封じた。

## [0.6.5] - 2026-11-20

### Added

- サイトヘッダに日英切替トグルを追加。テーマ切替の隣に常時表示し、
  ハンバーガー内にあった言語リンクはやめた。対訳ページへの素の
  リンク（JS 不要）で、英語のみの頁は日本語トップへ飛ばす。`head` に
  `rel=alternate` も付与する。

### Changed

- 依存クレート chrono をやめ、jiff へ置き換えた。chrono が
  RUSTSEC-2026-0543 で unmaintained 宣言され、`cargo deny` の依存ゲートが
  赤くなっていたため。RFC3339 表記・JST バケット境界・`--since` /
  `--last` の暦境界（±262,000年）・`cleanup` のローカル時刻表示は
  従来とバイト同一。

## [0.6.4] - 2026-11-17

### Added

- GitHub Pages サイト (`site/`)。導入・使い方・コマンド対応表・設計
  spec 全文・`--help` 生成の CLI リファレンスを日英で公開する。
  `main` への push で自動デプロイされる。

### Changed

- README / README.ja.md を導入から使い方の順に再構成した。リリース
  手順は CONTRIBUTING のリリース節へ移動した。
- 設計 spec の重複記述を整理した (cli.md 中心)。

## [0.6.3] - 2026-11-16

### Fixed

- kilocode 指示ファイルの項目名が stderr 警告にそのまま出て、改行の
  偽造や ANSI による端末表示の改変が可能だった問題を修正。制御文字は
  U+FFFD に置換される (VULN-001)。

## [0.6.2] - 2026-11-15

### Added

- advisor 言語の決定ソース表示。`--verbose` 実行時に言語を決めた層
  (flag / config / locale / default) を stderr に一行出す。通常出力と
  JSON はバイト同一のまま。
- `LANGUAGE` 環境変数による advisor 言語解決。`LC_ALL` > `LC_MESSAGES` >
  `LANG` がいずれも非日本語・非 `C` のとき、先頭要素のサブタグが `ja`
  なら日本語になる。実効ロケールが `C` / `POSIX` のときは無視する
  (gettext の C 優先に準拠)。
- `cleanup --list` の未読エントリ件数の可視化。読めないエントリがあっても
  一覧は失敗せず、件数を stderr 警告に出す。通常出力と JSON はバイト同一。

### Fixed

- `--apply` 拒否メッセージの harness 群表記を launch 表からの生成にし、
  将来の headless 追加で陳腐化しないようにした。空プラン失敗の文言から
  存在しない raw log 削除の主張を除いた。
- symlink 非解決方針の doc を `sized_path` に正典化し、他3箇所を参照に留めた。

## [0.6.1] - 2026-11-12

### Added

- advisor 言語の最終フォールバックとして locale 解決を追加。
  `--language` も設定ファイルも言語を名指ししないときは `LC_ALL` >
  `LC_MESSAGES` > `LANG` を見て、先頭が `ja` なら日本語、それ以外は
  従来どおりバイト同一の英語になる。

### Changed

- 内部整理（出力は不変）。`--apply` の headless ガード・順序比較子・
  サイズ計算・テスト用スクラッチ配置をヘルパーに集約し、locale 解決の
  文字列往復をやめた。ガードは deny-by-default（ワイルドカードなしの
  網羅 `match`）に戻し、公開 doc の帰属も正した。全テスト green。

### Fixed

- CI の flake とクロスプラットフォーム不整合を修正（タイムアウトテスト
  の余裕、XDG fixture の temp dir 化、golden の LF 固定ほか）。
  公開物・出力に変更はない。

## [0.6.0] - 2026-11-10

### Added

- 対話ガイドの最初にエージェント選択を追加。使う harness (claude /
  opencode / kilocode) を聞き、既定は解決値。`--harness` / `--profile`
  指定時は聞かずにその値を使う。
- ガイドの報告後に次の一手の実行を選べるようにした。`y` で同じ
  harness・scope の `optimize` をその場で実行し、失敗してもガイドは続く。
- ガイドの保存確認の文面を明確化。使っているエージェント名を添えて保存
  可否を尋ね、名前付き保存は `--profile` 名での呼び出しとして説明する。
- `optimize --apply` (headless のみ)。headless 実行の計画本文を取り出し
  て 0600 で保存し、同 harness の対話を開いて適用する。適用は項目ごとの
  予定 diff・解説・`y` / `n` / `q` で進み、触る前に backup を取り、最後に
  適用済みと提案のみのサマリを出す。headless 出力は先頭に提案のみの宣言を
  出す。
- `cleanup`: 保存プランと backup の一覧 (`--list`、引数なしも同じ) と世代
  掃除 (`--keep [N]`、`--keep` だけは既定 10)。ストアを開かずに動く。
- `--language en|ja`: advisor 指示文の出力言語を切り替える。表本体は英語
  のまま。ガイド内実行にも効く。
- ガイドの実行行を再現可能に。headless 時は実行行に `--apply` を含めて
  出す (`saniwalens optimize ... --apply`)。

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
