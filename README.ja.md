# saniwalens

[English](./README.md) | **日本語**

> この日本語版は、リリース（`vX.Y.Z` タグ）ごとに英語版へ追随します。
> 食い違いがあれば英語版が正です。

coding-agent の使い方を映すレンズです。`saniwalens` は agent のセッション
履歴（transcript）と現在の設定を読み、ローカルの SQLite ストアに取り込んで、
**時間・トークン・手間がどこで無駄になっているか、どう直せばよいか**を示します。
使われていない設定や重い設定、繰り返すツールの失敗、作業が行き詰まる箇所、
そしてその背後にあるコストの全体像が分かります。

設計意図は [`docs/specs/`](docs/specs/) にあります。この README は使い方の案内です。

## インストール

```sh
brew install armaniacs/saniwalens/saniwalens   # macOS / Linux
cargo install saniwalens                       # crates.io から。同梱の SQLite のビルドに C コンパイラが必要
nix run github:armaniacs/saniwalens -- doctor  # インストールせずに試す
nix profile install github:armaniacs/saniwalens  # PATH に入れる（末尾に /vX.Y.Z を付けるとリリースを固定）
```

Windows は、GitHub Release から自分のアーキテクチャ向けの
`-pc-windows-msvc` アーカイブを取るか、ソースからビルドします。ソースからの
ビルドと開発用タスクは [`CONTRIBUTING.ja.md`](CONTRIBUTING.ja.md) を参照してください。

## クイックスタート

```sh
# 1. まずここから。一画面の健康診断で、全体（~/.claude）と
#    プロジェクト別の所見に分けて表示します。初回は自動で analyze します。
saniwalens doctor

# 2. 直したい層に絞ります。
saniwalens doctor --scope global
saniwalens doctor --scope project:<slug>

# 3. 所見をもとに動きます。分析結果を渡した advisor セッションが、
#    原因を調べて具体的な直し方を出します。
saniwalens optimize --scope global
```

対話端末で `saniwalens`（または `saniwalens tui`）だけを打つと、同じ選択を
順に聞いてくれます。詳しくは[ガイド入力](#ガイド入力)を見てください。

### 読むもの、書くもの

分析は**読み取り専用**で、前回から変わった分だけを取り込みます。読むのは
transcript（`~/.claude/projects`）と現在の設定（`~/.claude/*` と、各プロジェクトの
`.claude/`・`CLAUDE.md`）で、書き込むのはストアだけです。外部には何も送りません。
`CLAUDE_CONFIG_DIR` が空でないパスに設定されていれば、Claude Code と同じく
`~/.claude` の代わりにそのパスを使います。

ストアはユーザーごとに 1 ファイルです。分析が、実行したディレクトリではなく
全プロジェクトにまたがるためです。既定の場所は
`${XDG_STATE_HOME:-~/.local/state}/saniwalens/saniwalens.db` で、すべての
コマンドが `--db <path>` で変更できます（プロジェクト内に置くなら
`.gitignore` に入れてください）。

読み取り系のコマンドは、報告の前にストアを自動で更新します。変わっていない
transcript は飛ばすので速く、更新時刻の行を stderr に出します。更新せずに今の
ストアをそのまま読むなら `--frozen` を付けます。`saniwalens analyze` を
自分で打つのは、読まずに更新だけしたいときです。

## コマンド

### パイプライン

| コマンド | すること |
| --- | --- |
| `saniwalens doctor` | 健康診断。全体とプロジェクト別に分けて表示します。**まずここから。** |
| `saniwalens analyze` | transcript と設定をストアに取り込みます（差分のみ）。読み取り系のコマンドが自動で呼びます。 |
| `saniwalens sql '<query>'` | ストアに対して任意の読み取り専用 SQL を実行します。 |
| `saniwalens optimize` | 所見を advisor セッションに渡し、改善を進めます。 |
| `saniwalens cleanup` | 保存済みの `--apply` 計画と backup を一覧（`--list`、既定）または整理（`--keep [N]`、既定は 10 件残す）します。ストアは不要です。 |

### ガイド入力

対話端末で引数なしの `saniwalens`（または `saniwalens tui`）を実行すると、
次の順に聞かれます。

1. 使う harness（`claude`・`opencode`・`kilocode`）。`--harness` か `--profile` を付けると聞かれません。
2. 見るもの（`doctor`・`waste`・`failures`）。
3. 範囲（scope）。

実行前に相当するコマンドを表示し（例: `saniwalens --harness kilocode waste
--scope global`）、実行後にもう一度表示します。次回以降はガイドなしで同じ
ことを再現できます。報告のあとは、同じ harness・scope で `optimize` を続けて
実行するか聞かれます。ここで失敗してもガイドは続きます。headless harness では、
表示されるコマンドに `--apply` が付きます。最後に、使った harness を
`default_harness` または名前付き profile として `config.toml` に保存するか
聞かれ、書き込む前に確認が入ります。

### 範囲（scope）

`--scope global | project | project:<slug>` は、`doctor`・`failures`・
`waste`・`inventory`・`optimize` の対象を、直したい層に絞ります。未使用や
重い設定は、ファイルの置き場所で振り分けます。繰り返す失敗は、過半数を
占めるプロジェクトのものとして扱い、特定のプロジェクトに偏らなければ
プロジェクト横断の癖として global に出します。

### ビュー

ビューは、素のクエリでは書けない分類・アルゴリズム・提案を含む定番の切り口です。
PR やメモに貼るなら `--format markdown`、機械で読むなら `--format json`
（全コマンド対応）を付けます。JSON のときは stdout に JSON だけが出て、更新の
お知らせは stderr に出ます。端末では色が付きますが、`NO_COLOR` を設定するか
パイプすると消えます。

```sh
saniwalens waste                 # 改善機会を順位付きで表示。推奨アクション付き
saniwalens inventory             # 入っている surface 全部（scope 付き）と実際の使用状況
saniwalens overhead              # セッションごとの常時読み込み量。設定と突き合わせ
saniwalens usage                 # skill 別の使用回数。多い順
saniwalens usage --by month      # 期間別（JST: year|month|week|day|hour）
saniwalens failures              # 繰り返すツール失敗を分類別に。直し方付き
saniwalens prompts               # 指示の出し方（steer/correct/question/instruct）
saniwalens stuck                 # agent が短時間に書き直し続けたファイル
```

### `sql`: ストアを直接読む

ビューにない切り口は SQL で引きます。クエリは引数でも stdin でも渡せます。
データベースは読み取り専用で開くので、クエリでストアを壊すことはありません。

```sh
saniwalens sql "SELECT category, tool, COUNT(*) n FROM tool_errors GROUP BY 1,2 ORDER BY n DESC"
echo "SELECT excerpt FROM tool_errors WHERE category='path-not-found'" | saniwalens sql
saniwalens sql "SELECT sql FROM sqlite_master"   # スキーマを確認
```

`tool_errors` ビューには `category`・`excerpt`・`tool`・`project` の列があります。

### `optimize`

`optimize` は harness 自前の advisor を、分析結果を渡して起動します。渡す内容は
非公開の一時ファイル経由で、コマンドラインには載りません。

- **対話型の harness**（`claude`。`--harness kilocode` なら `kilo --prompt`）:
  合意した内容をセッションが実際に適用します。
- **headless の harness**（opencode）: 計画を出力するだけで、何も変更しません。
  出力の先頭にその旨が出ます。`--apply` を付けると、計画を
  `$XDG_STATE_HOME/saniwalens/plans/` に保存し、その計画で harness の対話
  セッションを開きます。項目ごとに予定の diff が出て、`y`（適用）・`n`（飛ばす）・
  `q`（やめる）を選びます。触る前に対象ファイルをすべて `.../backups/` に
  写し、最後に適用した項目と提案のみの項目をまとめて表示します。

溜まった計画と backup は `saniwalens cleanup --keep [N]` で整理できます。
引数なしの `saniwalens cleanup` は一覧を出すだけです。

## 対応 harness

1 回の実行で分析する harness は 1 つで、`--harness` で選びます。既定は
`claude` なので、これまでの呼び方はそのまま動きます。

| `--harness` | Agent | Transcript | 設定カタログ | ストア | `optimize` の起動 |
| --- | --- | --- | --- | --- | --- |
| `claude`（既定） | Claude Code | `~/.claude/projects` | skills、rules、agents、MCP servers、`CLAUDE.md` | `saniwalens.db` | 対話型の `claude` |
| `opencode` | opencode | opencode データベース（`~/.local/share/opencode/opencode.db`） | commands、agents、`AGENTS.md` | `saniwalens-opencode.db` | headless の `opencode run` |
| `kilocode` | Kilo Code | エディタ `globalStorage` の task ファイル（Code / Cursor / VSCodium） | commands、agents、skills、`AGENTS.md`、`.kilorules`、`kilo.json` の `instructions[]` | `saniwalens-kilocode.db` | 対話型の `kilo --prompt` |

```sh
saniwalens analyze --harness kilocode
saniwalens doctor --harness kilocode
saniwalens optimize --harness kilocode
```

harness ごとにストアを持ち、設定も harness ごとのもの（`~/.claude/`、
`~/.config/opencode/`、`~/.config/kilo/`、各プロジェクトのファイル）を読むので、
所見が混ざりません。`auto` を用意していないのは、複数の harness が入っている
環境で推測すると、気づかないうちに別の harness の行として数えてしまうからです。
harness ごとの契約は [`docs/specs/harness.md`](docs/specs/harness.md) にあり、
各 harness がまだ報告できないこと（opencode / kilocode は subagent の内訳が
出ない、opencode は `prompts` ビューが無い、など）も書いてあります。

DSH（DeepSeek harness アプリ）とその他の harness は未対応です。次の候補は
DSH です。新しい harness を足すための条件は
[`CONTRIBUTING.ja.md`](CONTRIBUTING.ja.md) にあります。OpenAI 互換の endpoint が
あるだけでは足りず、transcript と設定の形式が観測でき、OSS で再現できる
必要があります。セキュリティの報告は [`SECURITY.ja.md`](SECURITY.ja.md) の
とおり非公開 advisory で受け付けます（公開 issue は使わないでください）。

## 設定ファイル

`~/.config/saniwalens/config.toml` は任意です。無ければ上で説明した既定の
とおりに動きます。

```toml
default_harness = "kilocode"
language = "ja"   # advisor の出力言語（optimize のみ）。表本体は英語のまま

[profiles.work]
harness = "claude"
db = "work.db"   # 設定ファイルのあるディレクトリが基準
```

`saniwalens --profile work doctor` で、その profile の設定で動きます。明示した
フラグは常にファイルより優先されます。`language` は `--language`、ファイル直下の
`language`、英語の順に決まり、今は `optimize` の促し文にだけ使われます。表本体は
常に英語です。解決順の詳細は [`docs/specs/cli.md`](docs/specs/cli.md) にあります。

## 分かること

- **設定 surface の全体像**: skills、rules、agents、MCP servers、
  `CLAUDE.md` / `AGENTS.md` を、起動時・オンデマンドのトークンコストと
  読み込み方式つきで台帳にし、実際の使用と突き合わせます。使われていないもの
  （消す）、常時読み込みで重いもの（痩せさせる）、コストが高いのに使用が
  まれなもの（絞る）を指摘します。
- **つまずきやすい箇所**: 繰り返すツール失敗を分類とツール別に（`failures`）、
  agent が書き直し続けたファイルを（`stuck`）、具体例と集中しているプロジェクト
  つきで示します。
- **トークンの行き先**: メインスレッドの skill 出力と subagent のコストを分け、
  後者は agent の種類別（入れ子の spawn を含む）に内訳を出します。さらに、
  読み取れる設定と突き合わせた常時読み込みの下限も示します。残りは
  system prompt・tools・MCP で、ファイルからは削れません。
- **指示の出し方**: steer・correct・question・instruct の割合。

## 注意

- 件数は順位付けのための使用シグナルで、**課金の台帳ではありません**。静的コストは
  トークンの見積りで、実測値ではありません。
- **ストアは消さないでください。** transcript より長く残ります。Claude Code は
  自前の保持期間で transcript を削りますが、saniwalens は取り込み済みの行を
  残すため、ストアには `~/.claude/projects` にもう無い履歴が入っています。
  cleaner に空にされかねない cache ではなく
  `${XDG_STATE_HOME:-~/.local/state}/` 以下に置くのはそのためです。バージョン
  アップで schema が変わっても、ファイルはその場で移行されます。
- ストアは analyze のたびに育ち、期間ビューは全体を読みます。読みを速くする
  index は、次に読み書きで開いたときにストア自身が作ります。`--frozen` と
  `sql`（読み取り専用）は作りません。大きさは
  `saniwalens sql "SELECT COUNT(*) FROM events"` で確認できます。
- 意図的に flat にしています（[`docs/specs/`](docs/specs/) 参照）。`loop` など
  meta-skill の入れ子は推定しません。transcript には入れ子と連続実行を区別する
  手がかりが無いため、各呼び出しはそれぞれ独立した span のまま扱います。
