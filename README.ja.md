# saniwalens（日本語）

**English** | [日本語](./README.ja.md)

> 同期方針: この日本語版はリリースごと（`vX.Y.Z` タグ時）に英語版へ追随
> する。英語版が正で、食い違いがあれば英語版が優先。

coding-agent の利用を見るレンズ。`saniwalens` は agent セッションの
transcript と
 live の設定を読み、ローカルの SQLite ストアに展開して、**時間・トークン・
手間がどこで無駄になり、どう直すか**を示す:使われていない・重い設定、
繰り返す tool 失敗、行き詰まり箇所、その背後のコスト像。

設計意図は [`docs/specs/`](docs/specs/) にあり、この README は動かし方。

## インストール / ビルド

Homebrew（macOS と Linux、Intel と arm):

```sh
brew install armaniacs/saniwalens/saniwalens
```

Cargo で [crates.io](https://crates.io/crates/saniwalens) から（ソースビルド。
bundled SQLite に C コンパイラが要る):

```sh
cargo install saniwalens
```

Nix（ toolchain の用意が要らない):

```sh
nix run github:armaniacs/saniwalens -- doctor   # 入れずに動かす
nix profile install github:armaniacs/saniwalens  # `saniwalens` を PATH に入れる
nix profile install github:armaniacs/saniwalens/vX.Y.Z  # 特定リリース
```

ソースから ― 開発ツールは Nix flake で固定、タスクは `just` 経由
（素の `make` もあり、Cargo だけで足りる):

```sh
nix develop          # 開発シェルに入る（固定の rust、just、sqlite、…)
just                 # タスク一覧
just check           # rustfmt --check + clippy（CI の関門）
just test            # cargo test
just build           # target/release/saniwalens にリリースバイナリ
```

```sh
make                 # ターゲット一覧（nix/just なしで同じ仕事）
make check           # rustfmt --check + clippy（CI の関門）
make test            # cargo test
make build           # target/release/saniwalens にリリースバイナリ
make install         # ~/.local/bin に入れる（PREFIX=/usr/local で変えられる）
```

素の Cargo でも動く（`cargo build --release`)。rust が新しければよく、
flake は固定するだけ。CI は `nix develop -c just check` / `just test` を回す。

`vX.Y.Z` タグで GitHub Release を出すと Linux・macOS・Windows（x86_64 と
arm64 ずつ）のバイナリがビルドされ、`.sha256` 付きでリリースに添付され、
[`armaniacs/homebrew-saniwalens`](https://github.com/armaniacs/homebrew-saniwalens)
tap が新アーカイブを向き、crate が crates.io に出る。リリースは成果物より
先に公開されるので、数分間は本文だけ出ている。

版はタグにだけ書く:リリースワークフローがビルド・公開の前に
`Cargo.toml` の `version` をタグから書き換えるので、コミット済みの
`0.0.0` は仮置きであり、リリースを切るのにファイルを上げる必要は無い。
flake もタグを知らずに刻む:`VERSION` は `export-subst` 印なので、タグの
アーカイブ自体がタグを持つ（
[nix-github-release](https://github.com/lambdalisue/nix-github-release) 参照)。

tap は Linux と macOS のみ。Windows はリリースから自分の arch の
`-pc-windows-msvc` アーカイブを取るか、ソースビルドする。
`CLAUDE_CONFIG_DIR` は空でないパスに置かれていれば設定ツリーを直接指す
（Claude Code 自体と同じ)。そうでない場合 ― 変数が空の場合も含め ―
`~/.claude` は `HOME` から探し、`HOME` の無い Windows では `USERPROFILE`
に落ちる。

## クイックスタート

```sh
# 1. まずここ ― 一画面の健康診断。直す先を全体 (~/.claude) と
#    プロジェクト別に分ける。初回は自動で analyze する。
saniwalens doctor

# 2. 直す層に絞る。
saniwalens doctor --scope global
saniwalens doctor --scope project:<slug>

# 3. 見つけた点を動かす:対話 `claude` セッションに渡し、
#    原因を調べて具体策を出させる。
saniwalens optimize --scope global
```

分析は transcript（`~/.claude/projects`）と live の設定 ―
`~/.claude/*` と各プロジェクトの `.claude/` / `CLAUDE.md` ― を
**読み取り専用**・増分で読み、ストアに書く。外に何も送らない。
ストアはローカルファイル。全コマンドが `--db <path>` を取り、既定は
`${XDG_STATE_HOME:-~/.local/state}/saniwalens/saniwalens.db` ― 分析対象が
実行場所でなく全部のプロジェクトにまたがるので、ユーザ単位で一つ。
あえてプロジェクト内に置くなら `--db ./saniwalens.db`（`.gitignore` すること)。
読み取りコマンドは報告の前に自動でストアを更新し（変わっていない
transcript は飛ばすので速い）、鮮度行を stderr に出す。あるがまま読む
なら `--frozen`。自分で `saniwalens analyze` するのは、読まずに更新だけ
したいとき。

## 対応 harness

`saniwalens` は 1 回の実行で 1 つの agent harness を分析し、`--harness`
で選ぶ。`0.5.4` はこの 3 つ。既定は `claude` のままなので、既存の呼び方は
バイト単位で変わらない。

| `--harness` | Agent | Transcript | 設定カタログ | ストア | `optimize` 起動 |
| --- | --- | --- | --- | --- | --- |
| `claude` (既定) | Claude Code | `~/.claude/projects` | skills、rules、agents、MCP servers、`CLAUDE.md` | `saniwalens.db` | 対話 `claude` セッション |
| `opencode` | opencode | opencode データベース (`~/.local/share/opencode/opencode.db`) | commands、agents、`AGENTS.md` | `saniwalens-opencode.db` | headless `opencode run` advisor |
| `kilocode` | Kilo Code | エディタ `globalStorage` の task ファイル (Code / Cursor / VSCodium) | commands、agents、skills、`AGENTS.md`、`.kilorules`、`kilo.json` `instructions[]` | `saniwalens-kilocode.db` | 対話 `kilo --prompt` セッション |

```sh
saniwalens analyze --harness kilocode      # エディタの globalStorage task ファイルを読む
saniwalens doctor --harness kilocode
saniwalens optimize --harness kilocode     # 対話 `kilo --prompt` セッションを開く
```

harness ごとにストアを持ち（表参照）、設定も harness ごと ―
`~/.claude/`、`~/.config/opencode/`、`~/.config/kilo/` と各プロジェクトの
ファイル ― を読むので、所見が混ざらない。`auto` はあえて無い:複数 harness
が入っていると推測が黙って誤帰属する。harness ごとの契約は
[`docs/specs/harness.md`](docs/specs/harness.md) にあり、各 harness の未対応
も書いてある（例:opencode/kilocode に subagent 分割が無い、opencode に
`prompts` ビューが無い)。

`0.5.4` の対象外:DSH（DeepSeek harness アプリ）とその他すべて。
DSH 対応が次の候補。[`CONTRIBUTING.ja.md`](CONTRIBUTING.ja.md) に新 harness
対応の条件がある（OpenAI 互換 endpoint だけでは足りず、transcript と設定
形式が観測可能・OSS 再現可能であること)。セキュリティ報告は
[`SECURITY.ja.md`](SECURITY.ja.md)（非公開 advisory。公開 issue 禁止)。

## 設定ファイル

既定は `~/.config/saniwalens/config.toml`（任意 ― 無くても従来どおり):

```toml
default_harness = "kilocode"

[profiles.work]
harness = "claude"
db = "work.db"   # 設定ファイルのあるディレクトリ基準
```

`saniwalens --profile work doctor` でその profile 動作。明示フラグは常に
ファイルより勝つ。解決順は [`docs/specs/cli.md`](docs/specs/cli.md)。

## コマンド

### パイプライン

| コマンド | すること |
| --- | --- |
| `saniwalens doctor` | 健康診断。全体 / プロジェクト別に分ける ― **まずここ**。 |
| `saniwalens analyze` | transcript + 設定をストアに展開（増分）。読み取りは自動で走る。 |
| `saniwalens sql '<query>'` | ストアへの任意の読み取り専用 SQL（後述）。 |
| `saniwalens optimize` | 所見を対話 `claude` セッションに渡して動かす。 |

`--scope global | project | project:<slug>`（`doctor`、`failures`、
`waste`、`inventory`、`optimize`）は報告を直す層に絞る:未使用・重い設定の
所見はファイルの居場所で振り分け、繰り返し失敗は過半数を持つプロジェクト
のもの ― そうでなければ横断の癖として全体に出る。

### ビュー

素のクエリでは書けない理屈（分類・アルゴリズム・提案）を持った切り口。
PR やメモに貼るなら `--format markdown`、型付き機械読みなら
`--format json`（`doctor` と `analyze` 含め全コマンド） ― stdout には
JSON だけ載り、鮮度の話は stderr。人の目向けは端末で色付き。
`NO_COLOR` かパイプで消える。

```sh
saniwalens waste                 # 改善機会を順位付きで。推奨アクション付き
saniwalens waste --scope global  # …全体設定の掃除で触る分だけ
saniwalens inventory               # 入っている surface 全部（層付き）× 実使用
saniwalens overhead               # セッション毎の常時読み込み。設定と突き合わせ
saniwalens usage                  # skill 別使用量。呼び出しの多い順
saniwalens usage --by month       # 期間別使用量（JST: year|month|week|day|hour）
saniwalens failures               # 繰り返す tool 失敗を分類別・順位付きで。直し方付き
saniwalens failures --scope global          # どのプロジェクトのものでもない失敗（横断の癖）
saniwalens failures --scope project:<slug>  # 1 プロジェクトの失敗（直しはその設定に）
saniwalens prompts                # セッションの回し方（steer/correct/question/instruct）
saniwalens stuck                 # Claude が短時間に書き直し続けたファイル
```

### `sql` ― ストア直読み

作り込みビューに無い切り口は SQL ― ストアが本体。引数か **stdin** で渡す:

```sh
# 失敗分類ごとの tool は?
saniwalens sql "SELECT category, tool, COUNT(*) n FROM tool_errors GROUP BY 1,2 ORDER BY n DESC"

# path-not-found 背後の実パス（stdin 形):
echo "SELECT excerpt FROM tool_errors WHERE category='path-not-found'" | saniwalens sql

# スキーマを知る:
saniwalens sql "SELECT sql FROM sqlite_master"
```

`tool_errors` ビューが失敗の列名（`category`、`excerpt`、`tool`、
`project`）を付ける。db は読み取り専用で開くので、クエリが導出ストアを
壊すことは無い。

## 対象

- **設定面ぜんぶ** ― skills、rules、agents、MCP servers、
  `CLAUDE.md` / `AGENTS.md` ― を起動時・オンデマンドのトークンコストと
  load mode 付きで台帳化し、
  実使用と突き合わせて unused（消す）、always-on heavy（痩せる）、
  costly+rare（絞る）を指す。
- **つまずき所** ― 繰り返す tool 失敗を分類・tool 別に（`failures`）、
  agent が書き直し続けたファイル（`stuck`）を、
  具体例と集中先プロジェクト付きで。
- **トークンの行き先** ― 本スレ skill 出力と subagent コスト、後者を agent
  種別に割った内訳（誰が使ったか。入れ子 spawn 込み）、読み取り可能な設定と
  突き合わせた常時読み込みの下限（残りはファイルから削れない system + tools
  + MCP）。
- **促し方** ― steer / correct / question / instruct の混ぜ具合。

## 注意

- 件数は順位付けの使用シグナルで、**課金台帳ではない**。静的コストは
  トークン見積りで、実測実行値では無い。
- ストアは transcript より長生き。Claude Code は自前の保持期間で刈るが、
  saniwalens は取り込んだ行を残す ― db は `~/.claude/projects` がもう持たない
  履歴を持つ。消すのはキャッシュ掃除でなく実損。置き場が
  `${XDG_STATE_HOME:-~/.local/state}/saniwalens/` なのは、掃除屋が空ける
  cache 下でないため。版上げの schema 変更はファイルをその場移行する。
  消す必要は無い。
- ストアは analyze のたび育ち、期間ビューは全部読む。
  読みを速くする index は次回読み書き open 時にストア自身が作る。
  `--frozen` / `sql`（読み取り専用）は作らない。大きさは
  `saniwalens sql "SELECT COUNT(*) FROM events"` で見る。
- `optimize` は harness 自前の対話セッションを上げる（既定 `claude`、
  `--harness kilocode` で `kilo --prompt`)。所見を種にし、briefing は
  非公開 temp ファイルに書く。コマンドラインには載せない。
- 未実装（[`docs/specs/`](docs/specs/) 参照):meta-skill（`loop`）の入れ子 ―
  transcript から親子を取る設計判断が未決。
