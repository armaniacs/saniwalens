# PBI: waste の glob 計算に上限を設ける

- 優先度: 順位 07 / 13 件中
- 検証コマンド: `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo test`

## ユーザーストーリー
clone したリポジトリを分析する saniwalens ユーザーとして、`paths:` に
`**` を大量に含む rule が混ざっていても、`waste` 表示が有界時間で終わる
状態がほしい。なぜなら分析ツールの表示系が入力内容の組合せ爆発で固まる
のは、crash DoS と同種の実害であり、「clone したリポジトリの内容から
到達できる DoS」はプロジェクトが実害として認めた脅威クラスだからだ
（pbi/archived/2026-11-08-03-fix-rules-symlink-recursion.md 12-13 行目）。

## ビジネス価値
- 本件は symlink 再帰 PBI が閉じた crash DoS の面ではなく、**ハング**
  （最悪指数時間）の面。k 個の `**` を含むパターンは最悪
  (セグメント数+1)^k のバックトラックになり、全 suffix 起点を試す
  matches_any がさらに攻撃コストを掛け算する
- 既存の閉じ範囲とは明確に区別される:
  `pbi/archived/2026-10-31-27-test-analyze-bench.md` の analyze ベンチは
  glob/waste 経路が対象外、`2026-11-08-03` は symlink 再帰（crash 面）
- 通常の rules（少数の現実的なパターン）では wedge 出力がバイト同一 —
  日常の価値を壊さないまま最悪ケースに上限を置く

## BDD受け入れシナリオ

```gherkin
Scenario: 悪意ある ** の連鎖を含む rule glob でも waste が有界時間で終わる
  Given `**/**/x/**/y` 種の ** を多数含む glob を持つ rule と
    And 深い多セグメントの read target がある
  When saniwalens が waste を表示する
  Then glob マッチングは設定した比較回数の予算内で完了する
  And 予算を超過した rule は never-fires 判定から除外される
  And その旨の警告が stderr に出る（黙って別挙動にしない）

Scenario: 通常の rule では wedge 出力がバイト同一
  Given 少数の現実的な glob を持つ通常の rule 配置がある
  When 修正前と修正後のバイナリで同じ入力から waste を表示する
  Then stdout がバイト同一である
  And 警告は出ない

Scenario: 大量の read target でも glob 経路への入力が上限に飽和する
  Given ウィンドウ内に上限を超える数の DISTINCT read target が記録されている
  When waste が never-fires の証拠を集める
  Then file_read_targets は既存 briefing リーダーと同じ上限慣習
    （saturating_limit）で LIMIT された結果を返す
```

## 受け入れ基準
- [x] `src/core/glob_match.rs` の `**` 展開がメモ化（Earley 的な
  set-of-positions）または再帰深さ/比較回数の予算化され、k 個の `**` を
  含むパターン × 多セグメント target の最悪ケースが有界時間で完了する
- [x] 予算超過時は stderr 警告を出し、当該 rule を never-fires 判定から
  除外する（「黙って別挙動」にしない）
- [x] `store.file_read_targets` に LIMIT が付き、既存 briefing リーダーと
  同じ `saturating_limit` / `range_params` 慣習に乗る
- [x] 通常の rules（少数の現実的なパターン）に対する waste 出力が
  修正前とバイト同一である
- [x] 悪意ある `**/**/x/**/y` 種パターン + 多セグメント target で
  有界時間を pin する自動テスト（ベンチ含む）がある
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` が green

## テスト戦略（t_wadaスタイル）
- 単体（core・純粋）: まず失敗するテストから。悪意あるパターン
  （`**/**/x/**/y` × 多セグメント path）で比較回数が予算内に収まることを
  固定する。既存の matches_one / matches_any テスト
  （src/core/glob_match.rs:94-146）が等価性の証人になる
- 単体（core）: 予算超過の検出。超過が呼び出し側に観測可能な形で返り、
  除外対象の rule を識別できること
- 単体（store）: file_read_targets の LIMIT。上限ちょうど・上限+1 の
  境界で行数が飽和すること
- 統合: rule × target × glob の全組合せ経路（waste → never_fires）を
  合成 fixture で通し、予算超過時に警告 + 除外が起きること
- 性能: 悪意あるパターンの有界時間をベンチ/example で pin
  （既存 analyze ベンチは glob/waste 経路が対象外のため、この面は本 PBI で閉じる）

## 実装アプローチ
1. `src/core/glob_match.rs` の `**` 分岐（56-59 行目）をメモ化 —
   match_segs の pattern index × path segment index の組を状態とする
   Earley 的な set-of-positions 化、または再帰深さ/比較回数の予算
   カウンタを純粋引数として導入（両方でもよい。実装時に確定）
2. `src/store.rs` の `file_read_targets` に LIMIT —
   `saturating_limit`（store.rs:540-542）+ `range_params`
   （store.rs:573-582）の既存慣習に合わせ、他 briefing リーダーと
   同じ姿勢にする
3. 予算超過 rule の never-fires 除外 + stderr 警告 — waste.rs の
   no-silent-coercion 契約（src/cli/views/waste.rs:55-58、
   unknown_load_modes と同じ姿勢）に沿って view 層で配線
4. 出力バイト同一性: 通常パターンでは結果不変。既存 waste の
   golden/テスト出力が証人

## 見積もり
1日。glob_match の状態設計（メモ化 or Earley 化 + 予算）が実質の作業で、
LIMIT 追加と警告配線は既存慣習への乗せ替えに留まる。

## 技術的考慮事項
- glob_match は core（純粋）— 予算カウンタも純粋引数として渡し、
  I/O・stderr を core に持ち込まない。警告は view 層（waste.rs）で出す
- メモ化の状態空間: matches_any が全 suffix 起点
  （src/core/glob_match.rs:30-36）を試すため、起点ごとに再計算する形では
  メモ化が効かなくなる。起点も含めた評価になることを設計に織り込む
- LIMIT の意味論方向: never_fires は「1つも match しなければ flag」なので、
  target を落とすと誤った never-fires が増える方向に効く。上限は現実的な
  ウィンドウより十分大きく取り、超過時の扱いは黙って決めず spec に明記する
- 予算超過時の除外は「その rule について証拠が得られなかった」ことの表明で、
  flag を出さない方向に倒す — src/core/surface.rs:401-410 の
  "no evidence, never an accusation" と同方向
- ロールバック不要: 通常ルールで挙動不変であり、単独 revert 可能な
  変更にする

## 実装者向け注記

### 現状コードの確認
（行番号は本 PBI 作成時に現行ツリーで確認済み）
- `src/core/glob_match.rs:56-59` — `**` の各出現が
  `(0..=segs.len()).any(|skip| match_segs(rest, &segs[skip..]))` で分岐する
  素再帰（メモ化なし）。k 個の `**` を含むパターンは最悪
  (セグメント数+1)^k のバックトラック
- `src/core/glob_match.rs:30-36` — matches_any が全 suffix 起点
  (0..segs.len()) を試し、攻撃コストを掛け算する
- 呼び出し経路: `src/cli/views/waste.rs:96` →
  `store.file_read_targets(window)` → `src/store.rs:1134-1148`
  （SQL は 1137-1142、`SELECT DISTINCT ... ORDER BY ...` で LIMIT 無し、
  ターゲット数上限なし）→ `src/cli/views/waste.rs:111` →
  `src/core/surface.rs:407-427` が rule × target × glob の全組合せで
  matches_any を回す。上限・タイムアウトなし
- globs の由来: 分析対象リポジトリの rules `paths:` →
  `src/adapter/config.rs:173-192`（rule_surface → rule_paths）。
  非信頼入力として扱う
- 既存の閉じ範囲: `pbi/archived/2026-10-31-27-test-analyze-bench.md` は
  analyze ベンチで glob/waste 経路が対象外。
  `pbi/archived/2026-11-08-03-fix-rules-symlink-recursion.md` は
  symlink 再帰（crash DoS）で別面

### 実装手順
1. 失敗するテスト（red）: 悪意ある `**` 連鎖パターン + 多セグメント
   target で、比較回数が予算内に収まる（または時間が有界である）ことを
   要求する core 単体テストを書く
2. glob_match.rs の `**` 展開をメモ化（Earley 的 set-of-positions）し、
   予算カウンタを純粋引数で導入（green）
3. 予算超過を表す返り値を matches_any → never_fires → waste まで配線し、
   waste.rs で stderr 警告 + 該当 rule の never-fires 除外
4. store.rs の file_read_targets に LIMIT（saturating_limit /
   range_params 慣習）
5. 既存 waste golden/テストでバイト同一性を確認し、
   cargo fmt / clippy / test を通す

### 落とし穴
- メモ化は match_segs の pattern index × path segment index の組で行う。
  suffix 起点を試す matches_any との組合せで状態空間が変わる点を
  見誤ると、メモ化したつもりで起点ごとに再計算して予算が効かなくなる
- ベンチテスト（悪意ある `**/**/x/**/y` 種パターン + 多セグメント
  target）で有界時間を pin すること。通常パスのテストだけでは
  最悪ケースが検出できない
- waste の golden（既存出力）はバイト同一の証人。通常パターンで
  1 バイトでも変わるのはリグレッション
- LIMIT の飽和は「証拠が減る」方向に効く（誤った never-fires が増える）。
  黙って LIMIT だけ足さず、上限超過時の扱いをセットで明文化する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test` がすべて green
- [x] 通常の rules に対する waste 出力のバイト同一性を確認済み
  （修正前後の diff 空）
- [x] 悪意あるパターンでの有界時間が自動テストで pin 済み
- [x] コードレビュー完了（spec 準拠 + コード品質）
- [x] ドキュメント更新済み（docs/specs への予算・LIMIT 慣習の記載を含む）
