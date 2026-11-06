# Guided TUI（対話ガイド）設計 ― v0.5.4 の目玉

- 日付: 2026-11-06
- ブランチ: `feature/guided-tui`
- 目的: `saniwalens` をサブコマンドなしで起動しても迷わないよう、
  典型的な利用パターンへ自然に導く逐次プロンプト型ガイドを追加する。

## 背景

現状 `command: Command` は必須のため、引数なし起動は clap のエラーで終わる。
README の Quick start（doctor → scope → optimize）を知らない新規利用者は
次の一手がわからない。ガイドは「覚えなくてよい入口」として全4手で
レポート表示まで連れていく。

## 決定事項（brainstorming より）

- 終点: TUI 内で結果を見られ、かつ相当するコマンド文も表示（再現性）。
- 入口: 引数なし＋TTY で自動起動。明示手段は `tui` サブコマンドのみ
  （`--tui` フラグは見送り）。非 TTY では従来どおりエラー。
- 形式: 逐次プロンプト型。既存 table 出力を再利用。新依存ゼロ。
- 順序: harness → 目的 → scope → 実行。
- 範囲: doctor／waste／failures の3本＋相当コマンド文の表示。

## §1 入口と起動条件

- `saniwalens`（サブコマンドなし）かつ stdin・stdout ともに TTY → ガイド起動。
- `saniwalens --harness kilocode` のようにグローバル flag 付きで
  サブコマンドなしの場合もガイド起動し、harness 選択はスキップして
  指定値を初期値にする。
- `saniwalens tui` → `Command` enum に追加する明示入口（TTY 必須は同じ）。
- 非 TTY（パイプ・CI・リダイレクト）ではガイドを起動せず従来の挙動を維持。
- 終了（`q`／メニューの「終了」）→ 何も実行せず終了コード 0（Ctrl-C は中断）。

## §2 会話フロー（全4手以内・既定値付きで Enter 連打可）

1. harness 確認（既定 claude。config→既定の解決は既存 `resolve_cli` と同順）。
2. 目的選択: 健康診断 (doctor)／無駄を探す (waste)／失敗を調べる (failures)／終了。
3. 範囲選択: 全体／global／project の3択（`--scope` に対応）。
4. 相当コマンド文を「実行: …」として先に明示 → 実行（`analyze` は既存どおり自動）。

実行後は相当コマンド文を再掲し、「もう1件見る? [y/N]」で継続可
（y なら目的選択に戻り harness 維持、N・空 Enter で終了）。

`optimize`／`sql` は対象外。実行後の「次の一手」行で
`saniwalens optimize --scope …` の存在だけ示す。

## §3 実行・表示の仕組み

- 同一プロセス内で対応する既存ハンドラを呼ぶ。新規レンダラは作らない。
  `--format` は聞かず table 固定。
- 選択→コマンド文の組み立ては純粋関数 `guide::equivalent_command()` に隔離。
  例: `(kilocode, waste, project)` →
  `saniwalens --harness kilocode waste --scope project`。
- harness 選択肢は claude／opencode／kilocode の3つのみ。
- レポート実行エラーは既存メッセージをそのまま出してガイド終了
  （リトライループなし）。

## §4 テスト戦略と非目標

- テスト: `equivalent_command()` の組み立て表テスト＋選択肢パースの単体テストを
  先に書く（純粋・I/O フリー、t-wada 式）。対話全体は stdin パイプ実行で
  「相当コマンド文が出る」ことだけ確認。既存レポート出力の正しさは
  各コマンドの既存テストが担保する。
- 非目標: `optimize`／`sql` のガイド内実行、`--format` 選択、期間フィルタ指定、
  リトライ・履歴・カラー装飾、Windows 固有対応（CI 行列でビルドが通ることのみ）。
- 仕様の置き場所: `docs/specs/cli.md` にガイド節を追記（実装と同一変更で）。
- 実装（view-addition 規約 `.claude/rules/cli-view-addition.md` に従う）:
  `src/cli/views/` を新設し `src/cli/views/guide.rs` にガイドを置く。
  同コミットで `cli/trend.rs` を `src/cli/views/` へ移動する。
  `Command::Tui` 追加＋引数なし時の分岐。`tui` は `--last` を持たないため
  `WHOLE_HISTORY`（`tests/cli_help_golden.rs`）に追加し、`PINNED` には加えない。
  `mod.rs` の行予算（`tests/cli_mod_line_budget.rs`）は実測に合わせて上げる。
- JSON golden は設けない（規約の例外、理由付き）: 対話入口に機械出力はなく、
  再現物は相当コマンド文であるため、`equivalent_command()` の全組合せ表テストで
  pin する。これを機械契約とする。

## config.toml の保存（v0.5.4）

- ガイド実行後、相当コマンドの再掲に続けて保存を聞く（3択: 保存しない /
  default_harness にする / profile にする）。
- default_harness: 使った harness を既定にする。既存ファイルがあれば
  `default_harness` 行だけ更新し、なければ新規作成する。他行・コメントは
  保持する（TOML パースは既存値の確認のみ。書き換えは行単位の編集とし、
  Serialize による全文書き換えはしない）。
- profile: 名前を入力させ `[profiles.<名前>]` に `harness` を保存する。
  既存セクションがあれば `harness` 行を更新し、なければセクションを追加する。
  名前は英数・`-`・`_`のみ。db / scope は保存しない。
- 書き込み前にパスと変更後の抜粋を表示して y/N で確認する。保存先の解決は
  既存 `session::config_file_path` と同じにする。親ディレクトリは作成する。
- テスト: 本文生成は純粋関数として表テストし、実書き込みは薄いラッパーに留める。

## 将来の拡張（v0.5.5 以後）

v0.5.4 のガイド範囲は doctor／waste／failures の3本＋config 保存に絞ることを宣言する。
v0.5.5 以後の版で、利用状況を見ながら以下の候補を順次取り込む（順序未定）。

- 残りのレポート系: usage／trend／inventory／overhead／prompts／stuck
- `sql` のガイド内実行（定型クエリの選択肢化から）
- `--format`・期間フィルタ（`--since` 等）の指定
- `optimize` 起動への導線（現行は「次の一手」行での言及のみ）
- 終了時の「次の一手」行の拡充（実行結果に応じた提案）

## 例（想定セッション）

```text
$ saniwalens
saniwalens ― やりたいことを選んでください（q で終了）

[1] 使っているエージェント: claude
[2] やりたいこと: 健康診断 (doctor)
[3] 範囲: 全体
実行: saniwalens doctor
…（既存の doctor table 出力）…
実行したコマンド: saniwalens doctor
もう1件見る? [y/N]
```
