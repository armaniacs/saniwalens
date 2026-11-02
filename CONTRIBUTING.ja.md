# CONTRIBUTING

<!-- README-I18N:START -->

[English](./CONTRIBUTING.md) | **日本語**

<!-- README-I18N:END -->

saniwalensへのコントリビュート手順と、とくに新しいエージェント(ハーネス)対応を提案するときの材料をまとめます。

## 前提:このプロジェクトの作法

saniwalensは仕様駆動・テスト駆動で作られています。コードを書く前に、次の3点を押さえてください。詳しくは`AGENTS.md`と`.claude/rules/`にあります。

- 仕様優先(spec first):`docs/specs/`が設計意図(なぜ・契約)を持ちます。振る舞いを変えるときは、同じ変更で owning specを更新します。対応表は`.claude/rules/spec-sync.md`にあります。
- テスト優先(test first):RedからGreenへ、t-wada式です。分析コア(recordsからeventsへ、集計)は純粋・I/Oなしに保ち、小さな合成fixtureで単体テストします(`.claude/rules/tdd.md`)。
- プライバシー重視:実トランスクリプト・実コンフィグには個人情報や秘密が混ざります。テストfixtureは合成のみとし、採取した実データ・生成DBはコミットしません(`.claude/rules/session-data-privacy.md`)。
- ユーザー向けの仕事はPBI優先:いつもの進め方(PBIを作って)で進めるときは、実装前に`pbi/YYYY-MM-DD-NN-<type>-<slug>.md`を書きます。実装はサブエージェント駆動で行います。詳しくは`AGENTS.md`を見てください。

## 開発環境

```sh
nix develop          # 固定のrust、just、sqliteなどが入る開発シェル
just                 # タスク一覧
just check           # rustfmt --check + clippy(CIの関門)
just test            # cargo test
just build           # target/release/saniwalensにリリースバイナリ
```

素のCargoでも動きます(`cargo build --release`)。CIは`nix develop -c just check`と`just test`を回します。

## IssueとPR

- バグ報告は、使った`--harness`、再現するコマンド、期待と実際の出力を書いてください。実トランスクリプトや実コンフィグそのものは貼らず、合成の最小例に置き換えてください。
- 新機能の提案は、先にIssueで相談してもらえると助かります。とくに新しいハーネス対応は下の節を読んでください。
- PRは小さく、1つの関心に絞ります。spec更新とテストを含めてください。

## 新しいハーネス対応を提案するとき

メンテナの利用状況を先に開示します。もともとClaude Codeを使い、Kilo CodeをVS Codeと合わせて使いはじめ、kiloの無料エンドポイント主体からOpenAI互換のエンドポイント(さくらのAI エンジン)へ移り、Opencode Goを使うようになりました。現状いちばん使っているのはopencode v2です。最近はDeepSeekハーネスのアプリ版を使いはじめたので、次の対応候補にしています。触っていないエージェントが多数あるため、対応してほしいエージェントのおすすめを歓迎します。Issueで次の形で教えてください。

- エージェント名と版(例:opencode v2、Kilo Code、DSH アプリ版)
- セッション記録の置き場所と形式(ファイルパス、JSONLかDBか、版で変わる点)
- 設定ファイルの置き場所と形式(skills、rules、agents、MCP、memory、`AGENTS.md`相当の有無)
- あなたがそのエージェントで毎回払っているコスト感(常時読み込みの量、重い定義など)

ただし、対応可否はエンドポイント互換では決まりません。OpenAI互換のエンドポイントを指定できるエージェントかどうかは、対応の条件ではありません。対応に必要なのは、次の3点です。

1. 観測可能な形式:セッション記録と設定ファイルの読み方が、実機がなくても分かること。ドキュメントか、再現手順つきの説明が必要です。
2. OSS由来の再現性:テストfixtureを合成で作れ、リポジトリに載せてCIで回せること。非公開のバイナリやアカウントがないと確認できない形式は、こちらで検証も保守もできません。
3. 継続の見通し:上流の形式変更に追従できること。形式が頻繁に割れるものは、adapter層の隔離(`docs/specs/architecture.md`、`.claude/rules/format-isolation.md`)だけでは支えきれません。

OSSでないエージェントや、OpenAI互換のエンドポイントを指定できないエージェントの場合、上記1から3の材料がそろわないことが多く、対応がむずかしくなります。材料がそろうなら、エンドポイントの種類やOSSかどうかだけで断ることはありません。まずIssueで材料をそろえ、fixtureの作り方から相談してください。

技術的な受け入れ条件は`docs/specs/harness.md`のForward compatibility節にあります。新ハーネスは`Harness`の列挙子、既定の入力ルートとストア名、形式spec、analyzer-version鍵つきのadapterモジュールを、`Harness::analyze`の後ろに足す形になります。Claude Code側の振る舞いは変えません。

## リリース時のお願い(メンテナ向けメモ)

版番号はタグだけが持ちます。`Cargo.toml`の`0.0.0`は仮置きで、リリースワークフローがタグから書き換えます。CHANGELOGのUnreleasedを版に倒し、READMEとspecと`--help`の文面が一致していることを確かめてからタグを切ります。
