# PBI: pure-join 移動の同一 PR 契約に tripwire 更新を 4 点目として明記する

## ユーザーストーリー
将来の移動 PR の実装者として、tripwire が赤くなったときに「何をすべきか」を
失敗メッセージから読み取りたい、なぜなら現行の 3 点契約は rule・architecture・台帳を
更新させるが tripwire 自身を挙げておらず、3 点を全部やっても赤のままで、最短の
緑化が「定数を空にする」という契約の自己無効化になるからだ。

## 優先度
- 順位: 9 / 全候補数 10
- RICEスコア: 8.0（Reach=1 / Impact=2 / Confidence=3 / Effort=0.5日）
- 根拠: 移動が先に着地すると moot になるため、移動 PR より前に置く必要がある。
  ⑩ の後に置き、tripwire 契約の文言型を揃える。

## ビジネス価値
- 移動着地時に tripwire の移動側を「store 在 -> core 在 + store 不在」へ反転する
  手順が、rule・spec・失敗メッセージの 3 箇所で一致する。
- 定数の空化・削除・墓標コメントによる見かけの緑化を禁止し、vacuous pass を防ぐ。
- 測定方法: 定数を空にすると赤、tripwire の移動側を反転すると移動前は赤・移動後は緑、
  失敗メッセージに 4 点が含まれること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、probe 3本）
- `.claude/rules/pure-join-placement.md:41-43`、`docs/specs/architecture.md:146-147`、
  `tests/pure_join_placement.rs:10-12`、失敗メッセージ `:75-78` の 4 箇所が
  同じ 3 点（rule 列挙、architecture core row、台帳 note）を列挙し、tripwire を
  挙げた箇所は 0。
- `FIRST_PATTERN_JOINS` を空にすると T1/T2 は 0 回転で緑（vacuous pass、is_empty
  assert 0 件）。`contains` は生テキストなので `// moved: pub fn skill_deltas` でも
  T2 を偽装できる。
- 移動後に構造的に obsolete なのは 3 テスト中 T2 のみ。T1 は rule の書き換え次第、
  T3 は移動検知力 0。
- 根本原因: 同一 PR 契約の適用範囲を「移動が陳腐化させる散文」として定義し、
  「移動が赤にする検知器」を範囲外に置いたこと。類似契約
  `cli-view-addition.md:28-31` は tripwire 定数更新を同一 PR 項目に入れており、
  家風の写し漏れ。

## BDD受け入れシナリオ

```gherkin
Scenario: 移動着地の失敗メッセージが 4 点を教える
  Given tripwire の移動側テストが赤になっている
  When 実装者が失敗メッセージを読む
  Then rule 列挙、architecture core row、台帳 note、tripwire 自身の 4 点が示される

Scenario: 定数を空にすると赤になる
  Given FIRST_PATTERN_JOINS を空にする
  When tripwire を実行する
  Then 非空 assert が落ち、vacuous pass しない

Scenario: 移動後は tripwire が core 在を確認する
  Given 4 つの join が core へ移動した
  When 移動 PR が tripwire の移動側を反転する
  Then store 不在と core 在の両方が assert され、移動前は赤・移動後は緑になる
```

## 受け入れ基準
- [ ] rule の同一 PR 契約を 4 点に拡張する。4 点目は「tripwire の移動側を反転する。
  定数の空化・エントリ削除・墓標コメントでの充足は禁止。廃止する場合は rule と
  architecture の 2 つの prose 錨を同時に書き換える」
- [ ] `tests/pure_join_placement.rs` の失敗メッセージを `MOVE_REMEDY` 定数に切り出し、
  4 点を含める。定数文言自体を assert する
- [ ] `FIRST_PATTERN_JOINS` が空でないことを assert するテストを追加する
- [ ] rule が名指すシンボル集合と `FIRST_PATTERN_JOINS` のシンボル集合が一致する
  ことを assert する（片方向の contains だけでなく）
- [ ] tripwire doc を 4 点に同期する
- [ ] `docs/specs/architecture.md` の pure-join 節に、移動時に検知器の方向が反転する
  事実を 1 段落で書く
- [ ] 台帳の deferral note に 4 点目を追記する
- [ ] 移動側テスト（T2）は本 PBI では反転しない。移動 PR まで現行のまま
- [ ] spec-sync の陳腐化（"even while they still sit in src/store.rs"）は別候補として
  記録し、本 PBI では触らない
- [ ] fmt/clippy/`cargo test --test pure_join_placement` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `cargo test --test pure_join_placement` の 3 テストが green

### 統合テスト
- 定数を空にすると非空 assert が落ちる（変異確認、コミットしない）
- rule から 1 シンボルを消すと集合一致 assert が落ちる

### 単体テスト
- `MOVE_REMEDY` が 4 点を含む
- rule のシンボル集合 == 定数のシンボル集合

## 実装アプローチ
- **Outside-In**: vacuous pass と 3 点メッセージの再現 -> 非空 assert と集合一致
  -> メッセージ定数化 -> rule / architecture / 台帳。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
0.5 SP（E=0.5日。RICE 8.0）

## 技術的考慮事項
- 依存関係: 移動 PR より先。移動が先に着地すると本 PBI は moot
- `contains` による墓標コメント偽装は AST なしでは完全に閉じない。残余リスクとして
  レポートに明記し、レビューで見る
- core の移動先は未決定。tripwire の反転案は「core のいずれかのファイルに定義が在り、
  store に無い」という形にし、特定ファイル名を固定しない
- 非機能要件: docs-charset（U+2014 禁止、`―` は U+2015）、prose gate の `.rs` 錨

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `.claude/rules/pure-join-placement.md:41-43`（3 点）、`:55`（tripwire の錨）。
- `tests/pure_join_placement.rs:28-33`（定数）、`:57-62`（T1）、`:66-80`（T2）、
  `:83-95`（T3）。
- `docs/specs/architecture.md:127-147`（pure-join 節）、`:99`（core row）。
- `pbi/2026-10-30-00-backlog.md:214-220`（deferral note）。
- 類似契約: `.claude/rules/cli-view-addition.md:28-31`。

### 実装手順
1. 非空 assert と集合一致 assert を追加（現行で緑、空にすると赤）
2. `MOVE_REMEDY` を切り出し、4 点を含める
3. rule の 3 点を 4 点に差し替える
4. tripwire doc と architecture を更新
5. 台帳の deferral note を更新
6. 検証（pure_join_placement、docs-charset、prose gate、フルスイート）

### 落とし穴
- T2 を本 PBI で反転しない。移動前は store 在が正
- 定数を空にするテストはコミットしない。変異確認だけ
- 4 点目の文言を rule とメッセージで同一にし、片方だけ直さない
- `.claude/rules/*.md` も docs-charset の対象。`―` を使い、em dash を避ける

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 定数非空と集合一致が pin されている
- [ ] 失敗メッセージが 4 点を教える
- [ ] rule / architecture / 台帳が 4 点に同期している
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] docs-charset clean
