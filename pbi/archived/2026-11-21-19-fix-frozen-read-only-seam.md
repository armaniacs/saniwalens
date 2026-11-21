# PBI: --frozen を optimize/guide 経路でも読み取り専用に通す

## ユーザーストーリー
`optimize --frozen` で解析なしに既存ストアから advisor を回す saniwalens
ユーザーとして、frozen 実行がストアを一切変更せず、ストアの古さが表示され、
ストアが無ければエラーで止まる状態がほしい。なぜなら「凍結して読むだけ」
と約束した経路が裏でディレクトリ作成・chmod・migration を行い、空ストアを
作ってしまうのは契約違反で、古い analyzed_at のまま advisor を起動されるのは
計測ツールへの信頼を根底から崩すからだ。

## ビジネス価値
- 順位: 05 / 13 件中
- 根拠: `optimize` アームは開く段で frozen を参照せず
  `Session::open_resolved`（ReadWrite）で開く（src/cli/mod.rs:535 →
  src/session.rs:91）。`Store::open` の ReadWrite アーム（src/store.rs:628-636）
  は親ディレクトリ作成（0700、:630-633）+ ensure_owner_only（chmod、:634）+
  migrate（:635 の from_connection）まで実行する。一方 open_read の frozen
  分岐（src/session.rs:113-120 → open_frozen :129-151）は ReadOnly =
  no create / no chmod / no migrate（src/store.rs:637-647。:638-640 の
  コメントが契約を明記）に通っており、同じ `--frozen` なのに経路が分裂している
- 最悪系: ストア未存在で `optimize --frozen` → 空ストアが作成され、
  空 catalog の briefing で advisor が起動する。さらに optimize 経路には
  fresh 度表示がなく（`eprint_freshness` の呼び出しは src/session.rs:118 と
  src/cli/mod.rs:743 の sql のみ）、凍結した古い analyzed_at のまま advisor が回る
- guide の inline 次の一手（src/cli/views/guide.rs:682-688）も同型:
  frozen=true を run_apply（:684）/ optimize（:686）に渡しながら、
  guide_session（:728-738）が :735 の open_resolved で ReadWrite に開き直す

## BDD受け入れシナリオ

```gherkin
Scenario: ストア未存在で optimize --frozen はストアを作らない
  Given ストアが存在しない状態ディレクトリがある
  When ユーザーが `optimize --frozen` を実行する
  Then ストア不在のエラーで終了し、文面に `analyze` を促す語が含まれる
  And 状態ディレクトリにストアのディレクトリもファイルも作られていない
  And advisor は起動しない

Scenario: 既存ストアへの optimize --frozen はストアを変更しない
  Given 分析済みのストアがある
  When ユーザーが `optimize --frozen` を実行する
  Then ストアファイルの内容・mtime・パーミッションが変化しない
  And stderr に open_read 経路と同一文面の fresh 度行が表示される

Scenario: 非 frozen の optimize は従来どおり refresh する
  Given 分析済みのストアがある
  When ユーザーが `--frozen` 無しで optimize を実行する
  Then analyze による refresh が走り解析行が表示される
  And 修正前のバイナリと stdout がバイト同一である

Scenario: guide の次の一手も読み取り専用で開く
  Given guide でレポートを表示し次の一手を y で進めた
  When inline optimize（または --apply handoff）が走る
  Then ストアは ReadWrite で開き直されない
  And ストアの mtime がレポート表示時から変化しない
```

## 受け入れ基準
- [x] `optimize` アームの frozen 経路が ReadOnly open（open_frozen 系）で開き、
  `Store::open(..., ReadWrite)` を呼ばない（解決段階の legacy-store move も含め無変更）
- [x] guide の inline 次の一手（guide_session → optimize / run_apply）が
  同様に読み取り専用で開く
- [x] `optimize --frozen` と `--apply --frozen` で fresh 度行
  （open_read 経路と同一文面）が stderr に表示される
- [x] ストア未存在の `optimize --frozen` がストアを作らず、
  absent-store エラーで終わる
- [x] 非 frozen の optimize / --apply の挙動と出力が修正前とバイト同一である
- [x] tripwire テスト: frozen 経路で `Store::open(..., ReadWrite)` /
  create_private_dir_all / ensure_owner_only / migrate_legacy_store が
  呼ばれないこと（PBI 2026-11-08-12 受け入れ基準第 1 項の延長）
- [x] spec 同期: docs/specs/cli.md の freshness 記述（:122-124 付近）に
  frozen optimize/guide の読み取り専用と fresh 度行を追記
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体（tripwire）: frozen 経路の open が ReadOnly モードであることを固定する。
  PBI 2026-11-08-12 が open_read に置いた frozen テストと同型のアサートを、
  optimize/guide が通る新しい糸口に置く。書き込みを「呼ばれない関数」で縛り、
  今後の経路追加で再発したらテストが落ちる形にする
- 単体（境界）: `meta("analyzed_at")` が取れない（スキーマが古い）ストアでの
  fresh 度行の挙動。session.rs:118 は `?` で伝播、sql（cli/mod.rs:742-744）は
  `if let Ok` で無視という既存の二形があり、optimize がどちらに寄せたかを固定する
- 統合: prepare_optimize の frozen 分岐の fresh 度表示を capture し、
  optimize（cli/mod.rs:609）と run_apply（src/cli/handoff.rs:370）の
  両呼び出し側で表示されることを確認
- E2E: ScratchDir 環境で (a) ストア未存在 + `optimize --frozen` →
  ストアが作られない (b) 既存ストアの前後で mtime・内容ハッシュ不変
  (c) guide_flow（`SANIWALENS_TEST_FORCE_TTY`）の次の一手後も mtime 不変。
  非 frozen の golden diff が空であることを同一セットで固定する

## 実装アプローチ
1. session.rs に frozen で開く糸口を出す。`open_frozen`（:129-151）は非公開で
  `store_path` から自前解決するため、`open_resolved`（:90-100）と対になる
  「解決済みパス → frozen open」の split を追加する（optimize アームの
  resolve→parse→open の順序契約を保つため）
2. cli/mod.rs の optimize アーム（:516-543）を frozen 分岐: frozen なら
  legacy move 無しの解決 + frozen open。非 frozen は現行の
  `resolve_path`（:533）+ `open_resolved`（:535）をそのまま維持
3. prepare_optimize（:570-590）の `if !frozen`（:575-579）に else を足し、
  frozen 時に `eprint_freshness(&freshness_line(...))`
  （session.rs:118 と同一文面）を流す。optimize と run_apply の
  両呼び出し側が自動で被覆される
4. guide.rs の guide_session（:728-738）を frozen open に切り替える
  （inline 次の一手は frozen=true 前提。:674-675 のコメントが意図を明記）
5. spec 同期（cli.md）と tripwire・E2E テストを同梱

## 見積もり
1日。経路分岐が主作業で、frozen 分岐の解決段階（legacy move を避ける）と
テストの固定が実質の作業。

## 技術的考慮事項
- spec 同期（docs/specs/cli.md:122-124 付近の freshness 記述）を本 PBI に
  同梱する。:120「--frozen suppresses the refresh」は読み取りビューの文脈なので、
  optimize セクション（:466-473 の "analyzes (unless --frozen)"）にも
  読み取り専用と absent-store 挙動を追記する。:119「"absent db" is an error
  only for `sql`」も frozen optimize が absent-store エラーになるため読み直す
- 非 frozen 経路はバイト同一を契約とするため、ロールバックは不要
  （revert で戻るのみ）
- `--apply --frozen` は check_apply_args（src/cli/handoff.rs:75-92）では
  拒否されない: frozen --apply は既存ストアから plan を生成するのが現仕様
  （run_apply の doc comment :353「refresh unless frozen」、frozen が
  prepare_optimize へ伝わる :370）。本 PBI はこの分割線を変えず、
  開くストアのモードだけ ReadOnly に寄せる
- run_apply は plan を plans dir（state home）に書くがストアには書かないため、
  ReadOnly の Session を渡して成立する

## 実装者向け注記

### 現状コードの確認
- `optimize` アーム: src/cli/mod.rs:516-543。:529 check_apply_args、
  :533 resolve_path（pure default なら migrate_legacy_store 実行 —
  src/session.rs:217-219）、:535 open_resolved が frozen を参照せず
  ReadWrite で開く（src/session.rs:91）
- `Store::open`: src/store.rs:626-649。ReadWrite アーム（:628-636）は
  create_private_dir_all（親ディレクトリ 0700 作成、:630-633）+
  ensure_owner_only（chmod、:634）+ from_connection 経由の migrate（:635）。
  ReadOnly アーム（:637-647）は「No create, no chmod, no migration」
  （:638-640）で absent db はエラー
- 対称経路: src/session.rs:105-124 の open_read。frozen 分岐（:113-120）は
  open_frozen（:129-151）→ store_path 解決（legacy move 無し、:135-140）→
  Store::open ReadOnly（:141-142）→ guard_harness（:143）→
  eprint_freshness（:118）
- prepare_optimize: src/cli/mod.rs:570-590。`if !frozen`（:575-579）のときだけ
  analyze + 通知行。frozen 時は何も表示しない。集計は全履歴
  （TimeRange::ALL、:580-581）
- fresh 度表示: `eprint_freshness`（src/cli/report.rs:371）/
  `freshness_line`（report.rs:382）。呼び出しは session.rs:118 と
  cli/mod.rs:743（sql）のみ
- guide: src/cli/views/guide.rs。inline 次の一手は :682-688 — frozen=true で
  run_apply（:684）/ optimize（:686）を呼ぶが、guide_session（:728-738）は
  :734 resolve_path、:735 open_resolved で ReadWrite に開く
- handoff: src/cli/handoff.rs。check_apply_args（:75-92）は frozen を
  チェックしない。run_apply（:357-397）は :370 で prepare_optimize に
  frozen を渡す
- spec: docs/specs/cli.md:120（--frozen は refresh 抑制）、:122-124
  （freshness ヘッダ）、:322（--frozen と sql は index を作らない）、
  :466-473（optimize usage と "analyzes (unless --frozen)"）;
  docs/specs/storage.md:129（ReadOnly opens (--frozen, sql)）

### 前回の修正との関係（PBI 2026-11-08-12）
PBI 2026-11-08-12（fix-frozen-mutates-store）は open_read の frozen 分岐
（src/session.rs:113-120）だけを read-only open に通し、受け入れ基準
「frozen 経路で `Store::open(..., OpenMode::ReadWrite)` が呼ばれないこと」
を満たした。ただし対象は読み取りビューの open_read 経路に限定されており、
同じ `--frozen` を受け取る optimize / guide inline / --apply は
`open_resolved`（ReadWrite）のまま残された。本 PBI はその残ったアームを
閉じるもので、tripwire テストは同 PBI の受け入れ基準第 1 項の延長線上に
置く。同 PBI が「frozen でも freshness 行の表示挙動が維持されること」を
基準にしたのと対称に、本 PBI は optimize 経路の fresh 度表示を新設する
（open_read と同一文面への寄せ）。

### 実装手順
1. session.rs: `open_resolved` と対になる frozen 版の split
   （解決済みパス → ReadOnly open → guard）を追加する。
   `open_frozen`（:129-151）と `store_path`（:255、非公開）の公開範囲を
   実装時に決める
2. cli/mod.rs: optimize アームを frozen 分岐。frozen は legacy move 無しの
   解決 + frozen open、非 frozen は現行経路をバイト同一で維持
3. cli/mod.rs: prepare_optimize に frozen 時の
   `eprint_freshness(&freshness_line(...))` を足す（session.rs:118 の
   呼び出しを流用）。optimize と run_apply の両方から見える位置に置く
4. guide.rs: guide_session を frozen open に切り替え
5. テスト: tripwire（単体）+ fresh 度 capture（統合）+
   absent-store / mtime 不変 / guide_flow（E2E）
6. spec: docs/specs/cli.md の freshness 記述と optimize セクションを同期

### 落とし穴
- `open_read` が取るのは filter ではなく analyze クロージャ
  （src/session.rs:111）。optimize の集計は prepare_optimize が全履歴
  （TimeRange::ALL、cli/mod.rs:580-581）で担うため、open_frozen をそのまま
  流用できれば集計範囲の照合は不要。ただし optimize アームの現行解決
  （resolve_path、migrate あり）と open_frozen の解決（store_path、
  migrate 無し）の差は埋めること: frozen 分岐は open だけでなく解決段階から
  migrate を避けないと無変更契約が穴になる
- prepare_optimize の呼び出し側は fn optimize（cli/mod.rs:609）と
  run_apply（handoff.rs:370）の両方。fresh 度表示を prepare_optimize 側に
  入れると両方が被覆されるが、guide inline が run_apply に frozen=true を
  渡す経路（guide.rs:684）でレポート直後の表示と重複しないかを E2E で確認する
- 解決→`--scope` パースの順序契約（cli/mod.rs:530-532 のコメント。
  二重失敗時に旧バイナリと同じエラーを出す）: frozen 分岐で解決方法を変えると
  frozen 時の失敗順が変わる。非 frozen の順序は維持し、frozen の順序変化は
  テストで固定して文書化する
- `meta("analyzed_at")` はスキーマが古いストアでエラーになり得る。
  session.rs:118 は `?` で伝播、sql（cli/mod.rs:742-744）は `if let Ok` で
  無視 — optimize がどちらに寄せたかを実装時に明記し、テストで固定する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` がすべて green
- [x] 非 frozen 経路の出力が修正前とバイト同一である（golden diff 空）
- [x] tripwire テストで frozen 経路の書き込み系呼び出し
  （ReadWrite open / mkdir / chmod / migrate）が不在であることを確認済み
- [x] spec 同期済み（docs/specs/cli.md の freshness・optimize 記述）
- [x] コードレビュー完了（spec 準拠 + コード品質）
