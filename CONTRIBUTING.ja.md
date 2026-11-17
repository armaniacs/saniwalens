# CONTRIBUTING

<!-- README-I18N:START -->

[English](./CONTRIBUTING.md) | **日本語**

<!-- README-I18N:END -->

saniwalens へのコントリビュート手順と、新しいエージェント（harness）の対応を
提案するときに用意してほしい材料をまとめます。

## 前提: このプロジェクトの作法

saniwalens は仕様駆動・テスト駆動で作られています。コードを書く前に、次の
4 点を押さえてください。詳しくは `AGENTS.md` と `.claude/rules/` にあります。

- **仕様が先。** `docs/specs/` が設計意図（なぜ・契約）を持ちます。振る舞いを
  変えるときは、同じ変更で対応する spec も更新します。対応表は
  `.claude/rules/spec-sync.md` です。
- **テストが先。** Red から Green へ、t-wada 式で進めます。分析コア
  （records から events、集計まで）は純粋に保ち、I/O を持ち込みません。小さな
  合成 fixture で単体テストできる状態を保つためです（`.claude/rules/tdd.md`）。
- **プライバシーは譲れない制約。** 実際の transcript や設定には個人情報や秘密が
  混ざります。fixture は合成データだけを使い、採取した実データや生成した DB は
  コミットしません（`.claude/rules/session-data-privacy.md`）。
- **ユーザー向けの変更は PBI が先。** 実装の前に
  `pbi/YYYY-MM-DD-NN-<type>-<slug>.md` を書き、実装はサブエージェントを使って
  進めます。詳しくは `AGENTS.md` を見てください。

## 開発環境

```sh
nix develop          # 固定の rust・just・sqlite などが入る開発シェル
just                 # タスク一覧
just check           # rustfmt --check + clippy（CI の関門）
just test            # cargo test
just build           # target/release/saniwalens にリリースバイナリ
```

nix や just なしで同じタスクを回すなら `make` も使えます（`make install` は
`~/.local/bin` に入れ、`PREFIX=/usr/local` で変更できます）。素の Cargo でも
動きます（`cargo build --release`）。CI は `nix develop -c just check` と
`just test` を実行します。

## Issue と PR

- **バグ報告:** 使った `--harness`、再現するコマンド、期待した出力と実際の出力を
  書いてください。実際の transcript や設定は貼らず、合成した最小の例に
  置き換えてください。
- **機能の提案:** 先に Issue で相談してください。新しい harness については
  次の節を読んでください。
- **PR:** 小さく、1 つの関心事に絞ります。spec の更新とテストも含めてください。

## 新しい harness の対応を提案するとき

メンテナはもともと Claude Code を使っていて、VS Code で Kilo Code も使い始め、
OpenAI 互換のエンドポイント（さくらの AI エンジン）を経て Opencode Go に
移りました。いま日常で使っているのは opencode v2 で、最近使い始めた
DeepSeek harness アプリ（DSH）が次の対応候補です。触っていないエージェントが
多いので、対応してほしいものがあれば教えてください。Issue には次を書いて
ください。

- エージェント名と版（例: opencode v2、Kilo Code、DSH アプリ版）
- セッション記録の置き場所と形式（パス、JSONL か DB か、版で変わる点）
- 設定ファイルの置き場所と形式（skills、rules、agents、MCP、memory、
  `AGENTS.md` に相当するものの有無）
- そのエージェントで毎回かかっているコストの実感（常時読み込みの量、重い定義など）

対応できるかどうかは、OpenAI 互換のエンドポイントを指定できるかでは決まり
ません。新しい harness に必要なのは次の 3 点です。

1. **観測できる形式。** セッション記録と設定ファイルの読み方が、実機がなくても
   分かること。ドキュメントか、再現手順つきの説明が必要です。
2. **OSS で再現できること。** fixture を合成で作れ、リポジトリに入れて CI で
   回せること。非公開のバイナリやアカウントがないと確認できない形式は、検証も
   保守もできません。
3. **続けていける見通し。** 上流の形式変更に追従できること。頻繁に壊れる形式は、
   adapter 層での隔離だけでは支えきれません
   （`docs/specs/architecture.md`、`.claude/rules/format-isolation.md`）。

クローズドなエージェントや OpenAI 互換のエンドポイントを持たないエージェントは、
この 3 点がそろわないことが多く、対応が難しくなります。ただし、エンドポイントの
種類やライセンスだけを理由に断ることはありません。まず Issue で材料を集めて
ください。最初の相談は fixture の作り方からで構いません。

技術的な受け入れ条件は、`docs/specs/harness.md` の Forward compatibility 節に
あります。新しい harness は、`Harness` の列挙子、既定の入力ルートとストア名、
形式の spec、analyzer-version 鍵つきの adapter モジュールを、`Harness::analyze`
の後ろに足す形で入ります。Claude Code の経路は変わりません。

## リリース（メンテナ向けメモ）

版番号はタグだけが持ちます。`Cargo.toml` の `0.0.0` は仮置きで、リリース
ワークフローがタグから書き換えます。CHANGELOG の Unreleased を版にまとめ、
README・spec・`--help` の文面が一致していることを確かめてから、タグを切ります。

`vX.Y.Z` タグで GitHub Release を公開すると、次の処理が走ります。

- Linux・macOS・Windows（x86_64 と arm64）のバイナリをビルドし、`.sha256` つきで
  Release に添付する。
- [`armaniacs/homebrew-saniwalens`](https://github.com/armaniacs/homebrew-saniwalens)
  の tap を新しいアーカイブに向ける（tap は Linux と macOS のみ）。
- crate を crates.io に公開する。

Release は成果物より先に公開されるので、数分間は本文だけが見える状態になります。
flake は、タグを教えなくても版を刻めます。`VERSION` が `export-subst` の印を
持っているため、GitHub が配るタグのアーカイブにタグ名が入るからです
（[nix-github-release](https://github.com/lambdalisue/nix-github-release) 参照）。
