---
title: "saniwalens v0.5.2をリリースしました: trendビュー、期間フィルタ、optimizeのtimeout"
emoji: "📝"
type: "tech"
topics: ["rust", "cli", "codingagent", "個人開発"]
published: false
---

<!--
下書きメモ（公開前に削除）
- 版数: v0.5.2。リリースノートURLはタグ発行後に有効(
https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2 )。公開前にリンク切れを確認する。
- 画像・GIFは未作成。本文中の <!-- GIF 位置 --> コメント位置に trend 実行画面などの短い録画を足す。
- 導入の困りごと1〜2文は仮文。公開前に実話へ差し替える。
- `published: true` にする条件: リリースノートURLの有効確認、版数・配布リンクの一致確認、画像の有無の確定。
- 初見向けドキュメントURL: https://github.com/armaniacs/saniwalens
-->

コーディングエージェントの設定の無駄を見つけるCLI、saniwalensのv0.5.2をリリースしました。利用者に見える変更は3つです。

- `trend`ビューの追加:直近の使い方の増減を、前期間との差分で出す
- 期間フィルタ(`--since` / `--until` / `--last`)の追加:8つのレポートビューを期間で絞れる
- `optimize`のheadless実行に`--timeout`を追加:期限超過で子プロセスをkillしてreapする

リリースノート: https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2

初めての方はこちら:

- ドキュメント: https://github.com/armaniacs/saniwalens
- 作者のZenn: https://zenn.dev/ar1

<!-- GIF 位置: saniwalens doctor と trend の実行画面の短い録画。未作成。 -->

## 対応ハーネス:今の3つと、入っていない1つ

v0.5.2が分析できるのは次の3つです。`--harness`で切り替え、既定は`claude`のままです。

| `--harness` | 対象 | 読み物 | 設定カタログ | ストア |
|---|---|---|---|---|
| `claude`(既定) | Claude Code | `~/.claude/projects` | skills、rules、agents、MCPサーバー、`CLAUDE.md` | `saniwalens.db` |
| `opencode` | opencode | opencodeデータベース | commands、agents、`AGENTS.md` | `saniwalens-opencode.db` |
| `kilocode` | Kilo Code | エディタglobalStorageのタスクファイル | commands、agents、skills、`AGENTS.md`、`.kilorules`、`kilo.json`の`instructions[]` | `saniwalens-kilocode.db` |

ハーネスごとにストアを分け、結果を混ぜません。`auto` detectorは意図的にありません。複数ハーネスが入っている環境で推測すると、行の帰属を黙って誤るためです。

DSH(DeepSeekハーネスのアプリ版)はv0.5.2に入っていません。次の対応候補として追っています。対応提案の材料は`CONTRIBUTING.md`に書きました。Issueで使っているエージェントのおすすめをもらえると助かります。

## cclensからのfork後に何が変わったか

Zenn向けに、fork後の経緯を少し詳しく書きます。出発点は`lambdalisue/cclens`の`v0.4.0`です。Claude Code専用の利用分析ツールでした。

最初の分岐は改名です。`agentlens`へ改名し、旧ストアの自動移行をつけました。その後`agentlens`がcrates.ioで同領域の別ツールに取得済みと分かり、再改名して`saniwalens`になりました。改名のたびに旧ストア(`cclens/`、`agentlens/`)を`saniwalens/`へ移動する処理を足し、削除も上書きもしない方針にしました。明示的な`--db`は移行対象外です。

v0.5.0で製品の向きを変えました。Claude Code専用のプラグイン機構(marketplace manifest、`plugins/cclens`パック)を削除し、マルチハーネス対応を入れました。Claude Codeは主戦場ではないという判断です。Claude専用の用途にはオリジナルの`cclens`を案内しています。文面もcoding-agent横断に汎用化し、リポジトリを`armaniacs/saniwalens`へ移し、Homebrew tapを`armaniacs/homebrew-saniwalens`にしました。`LICENSE`にforkの著作権表示を足し、元の表示は残しています。

v0.5.1は設定ファイル(`~/.config/saniwalens/config.toml`)対応と、`--version`がgitタグ由来の版を名乗る修正です。v0.5.2はその上の分析機能の追加で、内訳は次の節のとおりです。

## trendビュー:増減を見る

```sh
saniwalens trend
saniwalens trend --last 14d
saniwalens trend --since 2026-10-01 --until 2026-10-31
```

現在ウィンドウ(既定は直近7日)と、直前の同じ長さのウィンドウを比べます。種別ごとのイベント集計、スキル別の増減、セッション数とsubagent合計を、両ウィンドウの値と差分で行ごとに出します。

直前ウィンドウにイベントがない比較は「比較不能」と明示し、`-100%`のような誤読を生む表示は出しません。比較不能や実測0の理由は、JSONの`reason`文字列でも返します。既存のビューと`sql` / `analyze` / `optimize`の出力は変わりません。

<!-- GIF 位置: saniwalens trend の表出力の短い録画。未作成。 -->

## 期間フィルタ:8つのビューを期間で絞る

対象はusage、inventory、waste、overhead、prompts、failures、stuck、doctorの8つです。

| 欄 | 扱い |
|---|---|
| `--since <日時>` | この起点からの集計 |
| `--until <日時>` | この終点までの集計 |
| `--last <期間>` | 直近N日・時間・週(例:`7d`、`48h`、`2w`) |

引いたウィンドウはfreshness行・framing block・JSONの`window`で明示します。フラグなしの出力は従来とバイト同一です。`sql` / `analyze` / `optimize`は変更なしです。

期間クエリ用に`events_by_epoch`インデックスを足しました。既存ストアでは次回open時に作り、`SCHEMA_VERSION`は変えません。

## optimizeのtimeout:長い実行を切る

```sh
saniwalens optimize --print
saniwalens optimize --timeout 2m
```

headless実行に`--timeout <DURATION>`を足しました。`30s`、`2m`、`500ms`のような形式です。期限超過で子をkillしてreapし、エラーはraw logを名指します。既定は無効で、長い実行を勝手に切りません。

## 開発基盤の変更

利用者に見えない変更はここにまとめます。出力は従来と同一で、旧バイナリとのE2E出力269行がバイト同一、`cargo test` 391件がgreenです。

- 起動時解決(設定、フラグ、既定値、ハーネス、ストアパス、旧ストア移行、openとguard)を`Session`モジュールに集約
- storeのingestを1トランザクションの`IngestBatch` + `Store::ingest`に集約
- `analyze`のハーネス別dispatchを`Harness::analyze`に集約(4つ目のハーネスはadapter追加だけで通る)
- briefing組立を純粋な`core::findings`へ移設し、上限値を名前付き定数に
- 設定カタログ走査を`catalog_surfaces(harness, roots)`単一IFに集約し、サイト定義を`Site`のデータ行に
- `optimize`の配信を`deliver` seamに切り出し
- CLIを`cli/`配下へ分割し、windowed flagの宣言を1箇所に集約
- ライブラリAPI:`saniwalens::store`のtrend型群は`saniwalens::core::trend`へ移動(互換再輸出なし)
- `--last`の暦レンジ外丸め、`trend`のpoint kindの適用外metric行、`n/a`理由の不一致、heartbeatのstall後burstを修正。cadenceとpollに0を渡すとエラーに

## 入れ方

- GitHub Releases: https://github.com/armaniacs/saniwalens/releases
- crates.io: https://crates.io/crates/saniwalens
- Homebrew: `brew install armaniacs/saniwalens/saniwalens`

すでに入れている方は配布元から更新してください。Nixのflakeでも`github:armaniacs/saniwalens`から実行できます。

「困ったこと・欲しいこと」は使った`--harness`とコマンド、期待と実際の出力を添えてIssueで:

https://github.com/armaniacs/saniwalens/issues
