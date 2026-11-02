# PBI: spec-sync の pure-join 行から「store 在」の pin を外す

## ユーザーストーリー
仕様同期を担う開発者として、spec-sync の Code → Spec Map が移動予定のファイル位置を
主張しないでほしい、なぜなら pure-join の 4 点契約（rule / architecture / 台帳 /
tripwire）に載らない 5 点目の prose が残ると、移動 PR は必ずそれを直す羽目になり、
直し忘れれば spec が嘘になるからだ。

## 優先度
- 順位: 1 / 全候補数 9
- RICEスコア: 12.0（R=1 / I=1 / C=3 / Effort=0.25日）
- 根拠: 1 行の prose と tripwire 1 assert で閉じる quick win。同時に PBI 61 の move PR
  から「spec-sync も直す」という 4 点外の作業を消すので、依存の上流に置く。R=1 は
  「影響が 1 行に閉じる」の意で、価値は移動 PR のたびに効く。

## ビジネス価値
- pure-join 移動 PR の変更点が 4 点に閉じ、移動のたびに prose を探す作業が消える。
- spec-sync は concern → owning spec の対応表に戻り、位置の所有者が 1 つになる。
- 測定方法: `rg -n "still sit" .claude/rules docs/specs` が 0 件。spec-sync の当該行に
  `src/store.rs` が戻ると `cargo test --test pure_join_placement` が赤くなること。

## 背景（2026-11-02、round4 最終 whole-branch review M9）
- `.claude/rules/spec-sync.md:28` の行は、pure shaping の concern の説明に
  「core-bound by the placement rule even while they still sit in `src/store.rs`」を
  含む。この節は「移動前の現状」を書いており、移動が着地した瞬間に偽になる。
- 位置の所有者はすでに 2 つある: `.claude/rules/pure-join-placement.md`（Rules と
  「The 1st pattern, named」）と `docs/specs/architecture.md:99`（core row）/
  `:139-141`（still sit in `store.rs`）。PBI 51 はこの 2 つ + 台帳 + tripwire を
  4 点契約にした。
- spec-sync は 4 点に入っていない。`tests/pure_join_placement.rs` は rule
  （`:105-119`）と architecture（`:212-224`）を読むが spec-sync は読まない。よって
  この行だけは誰も落とせない。PBI 36 の prose gate（`tests/prose_reference_gate.rs`）
  は .rs 参照の存在だけを見るので、位置主張の真偽は見ない。
- 表の他の行は位置を書いていない（concern と owning spec だけ）。この 1 行だけが
  例外で、しかも「いまは store に居る」という時間依存の事実を書いている。
- 根本原因: 移動の契約を 4 点に閉じたとき、同じ事実を書いている 3 つ目の prose
  （spec-sync の表）を数え漏れたこと。

## BDD受け入れシナリオ

```gherkin
Scenario: 移動前でも spec-sync は移動先を主張しない
  Given spec-sync の Code -> Spec Map に pure shaping の行がある
  When その行を読む
  Then owning spec として `docs/specs/cli.md` を名指す
  And `src/store.rs` という位置主張を含まない
  And 位置は placement rule と architecture が所有することを 1 句で指す

Scenario: 移動後に spec-sync を触らなくてよい
  Given PBI 61 の move PR が tripwire の move side を反転した
  When `cargo test` を回す
  Then spec-sync を編集しなくても green
  And move remedy は 4 点のまま

Scenario: 位置主張を書き戻すと赤になる
  Given spec-sync の当該行に `src/store.rs` を書き戻した
  When `cargo test --test pure_join_placement` を回す
  Then 赤くなり、message が 4 点契約に揃えるか 5 点目にするかを名指す
```

## 受け入れ基準
- [ ] `.claude/rules/spec-sync.md:28` から「even while they still sit in
  `src/store.rs`」を削除する。行は 1 行のまま（表のセルなので折り返すと表が壊れる）。
  pure shaping の concern と owning spec（`docs/specs/cli.md`）は残し、「位置は
  `.claude/rules/pure-join-placement.md` と `docs/specs/architecture.md` が所有する」
  を 1 句で指す
- [ ] `tests/pure_join_placement.rs` に assert を 1 本足す。既存
  `the_spec_carries_the_placement_rule`（`:212-224`）を拡張するか、新 test
  `the_spec_sync_map_does_not_pin_the_join_location` を置く。内容は (a) spec-sync を
  読み、(b) 「Pure shaping of fetched rows」を含む行を特定し、(c) その行に
  `src/store.rs` と `still` が現れないこと、(d) `docs/specs/cli.md` を名指すこと
- [ ] 代替案 B を選ぶ場合のみ: 5 点目として spec-sync を rule の remedy に足し、
  `tests/pure_join_placement.rs:41-66`（MOVE_REMEDY）と `:170-210`
  （the_move_remedy_names_all_four_points）を 5 点に更新し、`docs/specs/architecture.md`
  の 4 点段落も 5 点にする。B を選んだら台帳に理由を 1 行残す
- [ ] 推奨は A（4 点に揃える）。spec-sync の表は concern の振り分けが仕事で、位置は
  他の行も書いていない。位置を 3 箇所に増やすと移動のたびに 3 箇所を直すことになる
- [ ] pure-join の移動そのものは実行しない（PBI 61 の範囲）
- [ ] `cargo test --test pure_join_placement`、fmt、clippy が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- なし（prose の契約は下の統合テストが cargo test の一部として回す）

### 統合テスト
- `tests/pure_join_placement.rs` の新 assert 1 本。`read(path)`（`:78-83`）と同じ流儀で
  repo のファイルを読み、対象行を特定してから部分一致を見る
- 変異確認: 行に `src/store.rs` を書き戻す -> 赤 / `docs/specs/cli.md` を消す -> 赤

### 単体テスト
- なし。文字列処理は上記 1 本に閉じる

## 実装アプローチ
- **Outside-In**: 新 assert を先に書き、現行 spec-sync で赤を確認してから 1 行を直す。
  赤の確認が「この行が今 pin されていない」ことの証明になる。

## 見積もり
0.25 SP（E=0.25日。RICE 12.0）

## 技術的考慮事項
- 依存関係: PBI 61 の move PR は本 PBI の後に置く（本 PBI が move を 4 点に保つ）。
  他の PBI とはファイルが重ならない
- 台帳の deferral note（`pbi/2026-10-30-00-backlog.md` の 214-223 行）は本 PBI では
  触らない。移動時に PBI 61 が書き換える
- spec-sync は `.claude/rules/` にあり prose gate のスキャン対象。`.rs` 参照を消すと
  gate の対象が 1 つ減るだけで、gate の assert は壊れない
- 非機能要件: `.md` は CP932 範囲、em dash 禁止（`―` を使う）

## 実装者向け注記

### 現状コードの確認（2026-11-02、HEAD 6dc84c4 で file:line 実証済み）
- `.claude/rules/spec-sync.md:28` が問題の行。表は `| concern | owning spec |` の 2 列。
- `tests/pure_join_placement.rs:32-39` の `FIRST_PATTERN_JOINS`、`:41-66` の
  `MOVE_REMEDY`、`:105-119` の `the_rule_names_the_trigger_symbols`（`:112` が
  `rule.contains("src/store.rs")`）、`:128-137` の move side、`:152-168` の集合一致、
  `:170-210` の 4 点 pin、`:212-224` の architecture pin。
- `docs/specs/architecture.md:99`（core row）、`:132-152`（4 点段落）。
- 既実装の確認: spec-sync の当該行に pin は無い（tripwire は spec-sync を読まない）。
  よって本 PBI は「pin を足す + 行を直す」の 2 手。

### 実装手順
1. Red: `tests/pure_join_placement.rs` に「対象行を特定して `src/store.rs` を含まない」
   assert を書き、現行で赤を確認
2. `.claude/rules/spec-sync.md:28` の行を書き換える（1 行のまま）
3. 代替案 A/B のどちらを採ったかを PR 本文に 1 行
4. 検証: `cargo test --test pure_join_placement`、`cargo test`、fmt、clippy

### 落とし穴
- 行を折り返すと表が壊れる。セル内は 1 行のまま長くてよい
- `src/store.rs` は spec-sync の他の行にも正当に現れうる（storage 行など）。行全体の
  `contains` ではなく「Pure shaping of fetched rows」を含む行に限定する
- `rule_named_symbols`（`:87-103`）は rule の bullet を切る実装で、表のセルには使えない。
  流用せず、行の特定は独自に行う
- 代替案 B を選ぶと 4 点が 5 点になり、PBI 61 の move PR の作業が増える。その分の
  見積もりを B の PR に載せる
- spec-sync の節は「更新義務」を定める文書なので、断定を弱めすぎない。「位置は rule と
  architecture が所有する」と所有を明示すれば、責務の移動であって情報の欠落ではない

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] `.claude/rules/spec-sync.md:28` が位置主張を含まない
- [ ] tripwire が spec-sync の位置主張の再混入を検知する
- [ ] PBI 61 の move PR が spec-sync を触らずに済む
- [ ] fmt、clippy、フルスイートが green
- [ ] コードレビュー完了
