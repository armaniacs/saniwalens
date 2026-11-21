# PBI: guide の TOML セクション認識を正当なヘッダ記法に対応させる

## ユーザーストーリー
saniwalens の config を手編集して使うユーザーとして、行末コメント付きの
テーブルヘッダ（`[profiles.work] # comment`）や行末空白を含む正当な TOML を
壊さずにガイドの保存ダイアログから更新できることがほしい。なぜなら現状は
そのようなヘッダを「未認知」として扱い、EOF に同名テーブルを追記した
二重定義の不正 TOML を書き戻し、次回以降の全起動をハードエラーにするからだ。

## ビジネス価値
- 優先度: 順位 08 / 13 件中
- 根拠: `src/cli/views/guide.rs:354-360` — table_header が
  `trim_start().strip_prefix('[')?.strip_suffix(']')?` で行末 `]` を硬要件とし、
  `[profiles.work] # comment` や行末空白付きヘッダを None（未認知）にする。
  upsert_toml_line では `trimmed.starts_with('[')`（:298）でセクション「閉じ」は
  起きるが SectionEnd の領域判定（:307-309、`table_header(raw) == path`）に
  入れず既存キーを置換しない → EOF で :335-340 が
  `\n[profiles.work]\nharness = ...` を追記し、元の同名テーブルと二重定義の
  不正 TOML になる。破綻経路は現実: 保存ダイアログはユーザーが手編集した
  config をそのまま読み（guide.rs:693-701、read_to_string → ask_save →
  render_profile → persist_config が書き戻し）。テーブルヘッダ行末のコメントは
  正当な TOML。書かれた不正 config は次回以降の全起動で config_file::load が
  ハードエラー（`src/adapter/config_file.rs:125-140`、「missing 以外はエラー」）。
  緩和: ダイアログの y/N 確認（guide.rs:436-443）はあるが、末尾に追記された
  重複セクションはプレビューの中で見落としやすい。

## BDD受け入れシナリオ

```gherkin
Scenario: 行末コメント付きテーブルヘッダのセクション内キーを置換する
  Given "[profiles.work] # comment" のような行末コメント付きヘッダと harness 行を含む既存 config.toml がある
  When  ガイドの保存で同じ profile に harness を上書きする
  Then  既存ヘッダ行とコメントは逐語保存される
  And   harness 行だけがセクション内で置換される
  And   [profiles.work] テーブルは出力内に 1 つだけ現れる

Scenario: 行末空白付きテーブルヘッダのセクション内キーを置換する
  Given "[profiles.work]   " のように行末空白を含むヘッダと harness 行を含む既存 config.toml がある
  When  ガイドの保存で同じ profile に harness を上書きする
  Then  harness 行だけがセクション内で置換される
  And   同名テーブルの二重定義は生じない

Scenario: 手編集 config への保存結果は正当な TOML として再読込できる
  Given 行末コメント付きテーブルヘッダを含む手編集の config.toml がある
  When  ガイドの保存で profile を上書きして書き戻す
  Then  書き戻されたテキストは TOML としてパースが成功する
  And   EOF への同名テーブル追記は行われない
```

## 受け入れ基準
- [x] table_header が行末コメント付き（`[profiles.work] # comment`）と
  行末空白付き（`[profiles.work]   `）ヘッダを正当なヘッダとして認識し、
  ヘッダ名 `profiles.work` を返す
- [x] 上記ヘッダの config で SectionEnd の領域判定が一致し、既存キー行が
  セクション内置換される（EOF への同名テーブル追記が起きない）
- [x] SectionEnd の EOF 追記経路（guide.rs:335-340）に「同名テーブルの
  開始行が既に存在する場合」のガードがあり、暗黙の二重定義を書かない
  （置換対象セクションとして扱うか、エラーで明示する）
- [x] 配列テーブル `[[x]]` と空 inner を None とする既存契約、
  内側パディング `[ profiles.work ]` の許容（既存 pin :1351-1363）が
  変わらない
- [x] 「`]` の後ろの `#` のみコメント扱いする」単純化の前提
  （ヘッダ行の `[` … `]` の間に文字列リテラルは出ない、キー行の値内 `#` は
  影響を受けない）がテストで pin されている
- [x] 正当な TOML ヘッダ 3 形（素 / 内側空白 / 行末コメント）+ 行末空白の
  pin テストと、手編集 config の round-trip（書き戻し結果が TOML パースを
  通る）テストがある
- [x] 変更は src/cli/views/guide.rs に閉じている（upsert_toml_line の
  !done ガードを含む既存骨格のテストは無変更のまま green）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features
  -- -D warnings / cargo test が green

## テスト戦略（t_wadaスタイル）
- 最初に失敗するテスト（単体）: table_header に行末コメント付きと行末空白付きの
  入力を与え、`Some("profiles.work")` を要求する。現行は None を返すので red
- 次に失敗するテスト（振る舞い）: 行末コメント付きヘッダの config で
  render_profile を通し、「ヘッダ行+コメントの逐語保存」「harness 行の
  セクション内置換」「同名テーブル 1 つ」を要求する。現行は EOF 追記で
  二重定義になるので red
- 仕上げの証人（round-trip）: 書き戻し結果を TOML パース（load_str 相当）で
  1 本検証し、「次回起動がハードエラーになる」という破綻経路の回帰を防ぐ
- 表で pin する境界: ヘッダ 3 形（素 / 内側空白 / 行末コメント）+ 行末空白、
  `[[x]]` と空 inner の None、キー行の値内 `#`（`harness = "a # b"` 相当）が
  逐語保存されること

## 実装アプローチ
- table_header（guide.rs:354-360）を「`[` 開始 → `]` まで走査、`]` の後ろの
  `#` コメント以降と末尾空白を無視してヘッダ名を抽出」に拡張する。
  既存契約は保持: inner が `[` 開始（配列テーブル）と空 inner は None、
  内側空白は trim で許容
- SectionEnd の EOF 追記（guide.rs:335-340）にガードを足す: 追記しようとする
  時点で既に同名テーブルの開始行が存在する場合は、置換対象セクションとして
  扱うか、エラーで明示する。どちらを採るかは実装者が決めてよい
  （エラー明示の方が保守的）
- upsert_toml_line の骨格（走査ループ :296-319、!done ガード :312/:318、
  insert_at :343-344）は変更しない — 変更は table_header と追記前ガードの
  2 点に最小化する

## 見積もり
0.5日。変更が guide.rs 内で完結し、red の起点が table_header 単体テストで
書けるため小さく見積もる。

## 技術的考慮事項
- 変更は src/cli/views/guide.rs 内で閉じる（table_header / upsert_toml_line は
  同ファイル内の private ヘルパー）
- 書き込み結果のバイト同一性は「正しく認識できた既存 config」についてのみ
  保証する。未認知だった config（行末コメント・行末空白付きヘッダ）への
  書き込み結果が変わること自体がこの PBI の目的であり、後戻りの対象にしない
- ロールバック不要: 旧挙動は「正当な TOML を不正 TOML に変換する」欠陥であり、
  戻す価値がない
- 行末コメントは正当な TOML。認識しない保守方針（例: 手編集で直す前提）は、
  書き戻し経路が存在する以上、破損を生むため採らない

## 実装者向け注記

### 現状コードの確認
- `src/cli/views/guide.rs:354-360` — table_header。
  `strip_suffix(']')?` が行末 `]` を硬要件化し、行末コメント・行末空白を
  未認知に落とす本次の犯人
- `src/cli/views/guide.rs:298` — upsert_toml_line の
  `trimmed.starts_with('[')`。セクション「閉じ」検出は trim_start 済み行、
  一方で領域判定は生行（raw）という非対称がある点に注意
- `src/cli/views/guide.rs:307-309` — SectionEnd の領域判定
  （`table_header(raw).is_some_and(|table| table == path)`）。
  ここで None になると in_region が立たず置換が起きない
- `src/cli/views/guide.rs:335-340` — EOF 追記（`\n[{path}]` + key 行）。
  二重定義の発生源。この経路にガードを入れる
- `src/cli/views/guide.rs:693-701` — 保存ダイアログの書き戻し経路
  （read_to_string → ask_save → persist_config）。手編集 config がそのまま
  入力になることを示す
- `src/cli/views/guide.rs:436-443` — y/N 確認。rendered プレビューに重複
  セクションが現れるが、緩和としては不十分（末尾の追記は見落としやすい）
- `src/cli/views/guide.rs:1351-1363` — 既存 pin
  （`profile_save_updates_a_whitespace_padded_header_in_place`）。
  内側パディングのみ対応済みで、行末コメント・行末空白は未カバー
- `src/adapter/config_file.rs:125-140` — parse / load。書かれた不正 config が
  次回起動時にハードエラーになる箇所（missing 以外はエラー）

### 実装手順
1. Red: table_header 単体テスト — `[profiles.work] # comment` と
   `[profiles.work]   `（行末空白）で `Some("profiles.work")` を要求する
   （現行は None で red）
2. Red: render_profile 振る舞いテスト — 行末コメント付きヘッダの config で
   「ヘッダ+コメントの逐語保存」「harness 置換」「同名テーブル 1 つ」を
   要求する（現行は EOF 追記で red）
3. Green: table_header を「`]` まで走査し、`]` 以降は `#` 以降（コメント）を
   無視して trim した inner を返す」形に拡張する
   （`[[` 開始と空 inner の None は維持）
4. Green: SectionEnd の EOF 追記前に同名テーブルの開始行が存在するかを
   確認するガードを足し、「二重定義を書かない」をテストで要求する
5. round-trip テスト: 書き戻し結果を TOML パース（load_str 相当）で検証する
6. 既存 guide.rs 内 tests が無変更のまま green であることを確認する
7. cargo fmt --check / cargo clippy --all-targets --all-features
   -- -D warnings / cargo test が green

### 落とし穴
- 文字列リテラル内の `#`（例: `name = "a # b"`）と行内コメントの区別。
  ただし table_header のヘッダ行判定では行頭 `[` … `]` の間にリテラルは
  出ないため、「`]` の後ろの `#` のみ見る」単純化で十分。この前提を
  テストで pin すること（キー行の値内 `#` をコメント扱いして削らないこと）
- upsert_toml_line の !done ガード（セクション内置換 × EOF 続行の回避、
  :312/:318）と干渉しないこと。契約を壊すと 1 回の保存で 2 箇所に書く、
  または置換が起きなくなる
- セクション「閉じ」検出（:298、trimmed 判定）と領域判定（:308、raw 判定）の
  非対称を保ったまま拡張する。両者でヘッダ解釈がずれると、閉じたのに
  in_region が立たない（= EOF 追記に転ぶ）回帰を生む

## Definition of Done
- [x] 全BDDシナリオが自動テストで実証される（table_header 単体 +
  render_profile 振る舞い + round-trip パース証人）
- [x] 正当な TOML ヘッダ 3 形（素 / 内側空白 / 行末コメント）と行末空白の
  pin が揃っている
- [x] 手編集 config を保存した結果が正当な TOML として再読込できる
  （パースで検証済み）
- [x] 変更が src/cli/views/guide.rs に閉じている
- [x] cargo fmt --check / cargo clippy --all-targets --all-features
  -- -D warnings / cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・
  spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（guide の config 書き込み契約に関する spec 記述が
  あれば同期し、該当がなければ更新不要と判断した）
