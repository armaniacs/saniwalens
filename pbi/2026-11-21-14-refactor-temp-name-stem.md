# PBI: 一時ファイル名生成の 3 重実装を 1 ヘルパーに統一する

## 優先度
- 順位: 14 / 全14件
- RICEスコア: 3.0（Reach=3 / Impact=0.5 / Confidence=100% / Effort=0.5日）
- 根拠: 「static AtomicU64 シーケンス＋SystemTime nanos＋pid で衝突しない一時名を
  作る」という同時実行制約を含むイディオムが 3 ファイルに同型で存在する。
  guide.rs のコメント自身が「`private_temp_path` と同じ idea」と明言しており、
  衝突耐性の修正（例: nanos のクロック解像度問題）が起きたとき 3 箇所の
  同時修正が必要になる

## ユーザーストーリー
メンテナとして、一時名生成の衝突耐性イディオムが 1 箇所にまとまっている状態が
ほしい、なぜならクロック解像度などの衝突耐性の修正が起きたときに 3 ファイルを
同時に直すのではなく 1 箇所だけ直せば済み、1 箇所だけの修正で性質が分岐する
リスクをなくしたいからだ。

## 背景
- 「衝突しない一時名」の同時実行制約イディオム（static AtomicU64 SEQ＋
  SystemTime nanos＋pid）が 3 ファイルに同型で重複している（現行の行番号で
  確認済み）:
  - `src/cli/deliver.rs:287-302` — `private_temp_path`（pid+nanos+SEQ を
    temp_dir に組む。288-292 のコメントがクロックと SEQ の役割分担を説明）
  - `src/cli/handoff.rs:132-139` — `plan_filename`（同型の SEQ/nanos/pid、
    plan-…md）
  - `src/cli/views/guide.rs:472-477` — `persist_config` 内の sibling 一時名
    （同型の SEQ/nanos/pid、.toml.tmp）。471 付近のコメントが
    `private_temp_path` との同型を自己申告
- 重複のコスト: 衝突耐性の修正が起きたとき 3 箇所の同時修正が必要になり、
  直し漏れや片側だけの修正で性質が分岐する
- 改善案: deliver.rs の `private_temp_path` を「一意サフィックス文字列を返す
  `unique_temp_stem()` ＋フォーマット」に分割し、3 箇所から呼ぶ
  （handoff/guide が deliver のヘルパーへ stem 生成を委譲）。生成される一時名の
  形式は現行とバイト同一（衝突耐性の性質を変えない）

## BDD受け入れシナリオ

```gherkin
Scenario: 一時名の連続生成が一意であり続ける
  Given 同一プロセス内で一時名を発行する経路がある（briefing / run log / plan / guide の sibling）
  When 短時間に連続で発行する
  Then すべての発行が互いに異なる名前になる
  And 同じ pid と同じクロック tick でも SEQ が名前を区別する

Scenario: 生成される一時名の形式が現行とバイト同一である
  Given リファクタ前のコードが発行する一時名の形式（deliver / handoff / guide の各形式）がある
  When リファクタ後のコードで同じ経路の一時名を発行する
  Then 各経路の生成名はリファクタ前とバイト同一の形式である

Scenario: guide の設定保存が一意な sibling 経由で行われ続ける
  Given guide の persist_config が config.toml を保存する経路がある
  When 2 つの guide（または同一プロセスの 2 テストスレッド）が並列に保存する
  Then それぞれが異なる一意な sibling 一時名に書き、rename で置き換える
  And 保存成功後に固定名の .toml.tmp 残骸は無い
```

## 受け入れ基準
- [x] deliver.rs の `private_temp_path` が「一意サフィックス文字列を返す
  `unique_temp_stem()` ＋フォーマット」に分割され、pid＋SystemTime nanos＋
  static AtomicU64 SEQ のイディオムが deliver.rs の 1 箇所になる
- [x] handoff.rs の `plan_filename` と guide.rs の `persist_config` が stem 生成を
  deliver のヘルパーへ委譲し、各ファイルの同型イディオム（static SEQ＋nanos＋pid）
  が消える
- [x] 生成される一時名が現行とバイト同一である（deliver / handoff / guide の
  既存テストが無変更で green）
- [x] 連続生成で一意であることの既存 pin（handoff.rs の
  `plan_filenames_are_unique_per_call` など）が流用され green
- [x] 衝突耐性のコメント意味論（クロック=leftover 回避、SEQ=同 tick の一意性、
  exclusive creation が本体）がヘルパー側に統合され、guide.rs:469-471 の
  自己申告コメントが解消される
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略
- 単体: stem 生成の連続呼び出しで一意性を固定する。既存 pin があれば流用する
  （handoff.rs:527 `plan_filenames_are_unique_per_call`）
- 統合: save_plan / persist_config / write_private_tempfile・run log の各経路で、
  既存テスト（guide.rs:1457 `persist_config_round_trip_through_a_pid_unique_dir`
  の round trip＋残骸なし、deliver.rs の delivery テスト、
  headless.rs:778 run log テスト）が無変更で green であること
- E2E: `cargo test` 全体。内部リファクタのため既存 golden と出力は不変。
  生成一時名の形式の証人は既存テスト（deliver / handoff / guide）

## 見積もり
0.5日（RICE の Effort と同一根拠）。機械的な抽出が主で、3 経路のバイト同一確認が
実質の作業

## 実装者向け注記
- **着手順序（重要）**: 同日の PBI 12（optimize-apply-prologue）が handoff.rs を
  触るため、**12 の完了後に着手する**（backlog のファイル排他: 14 は 12 の後）
- 現行の 3 実装（成分は同一: pid / SystemTime nanos / static AtomicU64 SEQ。
  ただし成分順・区切り・基数が異なる）:
  - `src/cli/deliver.rs:287-302` — `{prefix}-{pid}-{nanos:x}-{seq}.{ext}`
    （temp_dir 配下、nanos は 16 進）。呼び出し元: deliver.rs:308（briefing）、
    `src/cli/headless.rs:55`（run log）、`src/cli/mod.rs:35` で再エクスポート
  - `src/cli/handoff.rs:132-139` — `plan-{nanos:x}-{pid}-{seq}.md`
    （nanos 先頭）。save_plan（handoff.rs:147）が使用
  - `src/cli/views/guide.rs:472-477` — `{pid}.{nanos}.{seq}.toml.tmp`
    （nanos は 10 進、区切りはドット）を with_extension で適用（478）
  → 1 本の整形済み文字列では 3 形式を再現できないため、バイト同一を既存テストで
  検証しながら、呼び出し元のフォーマットが現行バイトを正確に再現する分割にする
  （統一するのは stem 生成のみ）
- 既存テストの証人: handoff.rs:527 `plan_filenames_are_unique_per_call`
  （連続一意性 pin、そのまま流用）/ guide.rs:1457
  `persist_config_round_trip_through_a_pid_unique_dir`（round trip＋残骸なし）/
  deliver.rs の delivery テスト（496-497、558-559 のポインタ経路）/ headless.rs:778
  `headless_failure_keeps_the_run_log_and_names_it_in_the_error`
- 衝突耐性の意味論は deliver.rs:288-292 のコメントのとおり（クロックは leftover
  回避のためで unguessability ではなく、exclusive creation が本体、SEQ が同 tick の
  一意性を担保）。統一後もこの性質を変えない
- 動作仕様の変更は無い想定のため spec-sync の該当は無いはず。docs/specs 側に
  一時名の記述があれば同梱更新する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` がすべて green
- [x] 生成一時名が現行とバイト同一であること（既存テスト無変更）を確認済み
- [x] PBI 12 の完了後に着手したこと（handoff.rs のファイル排他）を確認済み
- [x] コードレビュー完了（spec 準拠 + コード品質）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
