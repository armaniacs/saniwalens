# PBI: AdvisorLanguage の語彙を単一ソース化する

## ユーザーストーリー
saniwalens を保守する開発者として、アドバイザ言語の語彙（valid 値の一覧、
ラベル、エラーヒント、help 文言、decision 行のコード表記）が
`AdvisorLanguage` 型の一箇所から派生してほしい。なぜなら 3 言語目を
追加する際に、parse の match・エラーヒントの `(en | ja)`・decision 行の
コード match・help の `en (the default) | ja` という複数の手綴りを
すべて見つけて直す必要があり、一箇所でも漏れると無効値が通ったり、
エラーが正しい候補を案内できなくなるからだ。

## 優先度
- 順位: 04 / 全14件
- RICEスコア: 4.0（Reach=4 / Impact=0.5 / Confidence=100% / Effort=0.5日）
- 根拠: 先行の `Harness` は `ALL` + `label_list()` + parse derived で語彙を
  単一ソース化したが、後発の `AdvisorLanguage` は同じ定型を複数箇所で
  手綴りしており、3 言語目の追加時に漏れうる。ロケール解決も言語名固定の
  述語（`locale_is_japanese` の `subtag == "ja"`）と「ロケールは Ja にしか
  投票しない」という 2 値前提で書かれている。参照パターンが既に
  リポジトリ内にあるため Confidence は高く、Effort も軽い。

## BDD受け入れシナリオ

```gherkin
Scenario: 語彙は AdvisorLanguage 型の単一ソースから派生する
  Given AdvisorLanguage に ALL / label() / label_list() が備わっている
  When parse・エラーヒント・decision 行・help 文言を検査する
  Then それぞれが ALL または label() から派生しており、言語コードの
       手綴りがどこにも残っていない

Scenario: 不明な言語値のエラーヒントが有効一覧を正しく案内する
  Given --language に未定義の値 "fr" を渡す
  When CLI がエラーを出す
  Then メッセージは "unknown language 'fr' (en | ja)" であり
       候補一覧は label_list() の出力と同一である

Scenario: decision 行と help 文言はバイト同一を保つ
  Given --verbose を付けて言語を解決する呼び出しと saniwalens --help
  When 既存の golden（decision 行・help 文言）と突き合わせる
  Then 出力はリファクタ前とバイト同一である

Scenario: ロケール投票がサブタグ対照表で解決される
  Given LC_ALL / LC_MESSAGES / LANG / LANGUAGE の各候補値
  When resolve_locale_language に渡す
  Then サブタグ→言語の対照表（例: [("ja", AdvisorLanguage::Ja)]）経由で
       解決され、"ja_JP.UTF-8" は Some(Ja)、"en_US.UTF-8"・"C"・未設定は
       従来どおりの結果になる
```

## 受け入れ基準
- [x] `AdvisorLanguage` に `ALL`（canonical 順の定数）と `label()` と
  `label_list()` が備わる（`Harness` の型と同じ形）
- [x] `AdvisorLanguage::parse` は `ALL` を走査して `label()` 比較する形に
  派生し、言語コードの個別 match が消える
- [x] エラーヒント（src/session.rs:272 付近の `(en | ja)`）と decision 行の
  言語コード match（src/session.rs:182-185 付近）は `label()` /
  `label_list()` から派生する
- [x] help 文言（src/cli/mod.rs:67-69 付近の `en (the default) | ja`）は
  単一ソースから派生する（doc comment の静的展開など、clap の制約に
  応じた形でよい）
- [x] ロケール投票はサブタグ→言語の対照表に置き換えられ、
  `resolve_locale_language`（src/session.rs:118-141 付近）と受け側の
  match（src/session.rs:252 付近）が言語非依存の形になる
  （`locale_is_japanese` の `subtag == "ja"` 固定が消える）
- [x] 出力はバイト同一: 既存 help 文言・decision 行・エラーヒントの
  golden を pin してから変更する
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略（t_wadaスタイル）

### 単体テスト
- `AdvisorLanguage::ALL` / `label()` / `label_list()` の対称性:
  全 variant が parse で往復すること、`label_list()` が
  `"en | ja"` であること
- `resolve_locale_language` の対照表経由テスト: `ja_JP.UTF-8`・
  `ja`・`JA`（大文字小文字）・`en_US.UTF-8`・`C`・`C.UTF-8` + `LANGUAGE=ja`・
  未設定の各境界を既存テストで固定しつつ、対照表化で意味論が
  変わらないことを確認する

### 統合テスト
- 不明 `--language` 値のエラー文（`unknown language '...' (en | ja)`）の
  golden、`--verbose` の decision 行（`language: ja (decided by ...)`）の
  golden がリファクタ前後で一致すること

### E2Eテスト
- `saniwalens --help` の language 行がバイト同一であること
  （既存の help 文言 pin が等価性の証人）

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 参照パターンは `Harness` 側に既にある: src/adapter/harness.rs:442-452 付近
  （`ALL` 定数と `label_list()`）と :489-493 付近（parse を `ALL` 走査 +
  `label()` 比較で派生）。`AdvisorLanguage` は src/core/optimize.rs:173-179
  の 2-variant enum で、parse の match が :185-191 付近、
  `instruction_sentence` が :197-202 付近にある
- エラーヒントの手綴り: src/session.rs:272 付近
  `format!("unknown language '{v}' (en | ja)")`。
  直上の `parse_harness`（:280-286 付近）が `Harness::label_list()` を
  使う正しい形なので、それに揃える
- decision 行の言語コード match: src/session.rs:182-185 付近の
  `language_decision_line` 内。`AdvisorLanguage::label()` を使う形にする
- help 文言: src/cli/mod.rs:67-69 付近の `--language` doc comment
  `en (the default) | ja`。clap の doc comment は文字列リテラルを要求する
  ため、`concat!` + `include_str!` か、テストで help 出力と `label_list()`
  の一致性を assert する形のいずれかで単一ソース性を担保する
- ロケール投票の Ja 専用性: `locale_is_japanese`（src/session.rs:85-91 付近）
  が `subtag == "ja"` を手綴りし、`resolve_locale_language`
  （src/session.rs:118-141 付近）が `Some(Ja)` を直接組み立て、
  受け側（src/session.rs:252 付近）が
  `None if locale == Some(AdvisorLanguage::Ja)` で 2 値前提の照合を
  している。対照表 `[("ja", AdvisorLanguage::Ja)]` を導入し、
  述語・組立・照合を表から派生させる。`locale_is_c` は gettext の
  C priority という別の関心事なので手を付けない
- 出力のバイト同一性が本 PBI の契約: 先に help・decision 行・エラー文の
  golden を pin してからリファクタする。`instruction_sentence` の
  「En は None、Ja のみ文を足す」設計は En が命令を運ばない前提で
  成り立っているため、ALL 化の際もこの性質は保つ
- 落とし穴: `--language` と config file の `language` の両方が parse を
  経由する（src/session.rs:268-275 付近の `parse_language`）ため、
  parse を derived にした際に両経路のエラーメッセージが変わらないこと
  を確認する。spec-sync 該当（docs/specs/cli.md の language 節）があれば
  `.claude/rules/spec-sync.md` に従い同梱更新する
- エビデンス: docs/specs/cli.md（--language の契約）、
  src/session.rs の既存 locale テスト、help 出力の pin テスト

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] 言語コードの手綴り（`"en"` / `"ja"` のリテラル比較・文言）が
  src/core/optimize.rs と src/session.rs から消え、単一ソースのみを参照する
- [x] help・decision 行・エラーヒントの出力がリファクタ前とバイト同一
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test がすべて Green
- [x] コードレビュー完了
- [x] ドキュメント更新済み（spec-sync 該当があれば）
