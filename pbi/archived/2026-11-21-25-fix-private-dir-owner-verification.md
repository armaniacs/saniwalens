# PBI: private ディレクトリ・ストアの所有者異常を可視化する

## ユーザーストーリー
マルチユーザホストで saniwalens を使うユーザーとして、private を保証するはずの
状態（plans / backups / store）が他人所有・他人書き込み可能なディレクトリや
ファイルに置き換わっていたら、黙ってそれを使わず警告してほしい。なぜなら
「前提が破られたとき黙って続行しない」のが本ツールの privacy 姿勢であり、
攻撃者に plan の差し替えや world-readable ストアへの書き込みを許したまま
気づかずに済ますのは、0600 を守る取り組み自体を無意味にするからだ。

## ビジネス価値
- 優先度: 順位 11 / 13 件中。単一ユーザのデフォルト環境では顕在化しない
  防御的堅牢化であり、RICE の観点では低め。一方で privacy を売りにするツールの
  姿勢表明としての価値がある
- 現状: `create_private_dir_all` は既存ディレクトリの所有者・モードを検証しない
  （doc が「Directories that already exist keep theirs」と明言、
  `src/cli/deliver.rs:367` / `src/store.rs:471`）。`save_plan`
  （`src/cli/handoff.rs:131-136`）も再検証なし
- 脅威: `state_base` は絶対値の `$XDG_STATE_HOME` を何でも受理するので
  （`src/session_env.rs:311-327`）、マルチユーザホストで共有ディレクトリを
  指すと、攻撃者所有の plans ディレクトリをそのまま使う。攻撃者は 0600 の
  plan を読めないが、ディレクトリ所有者として unlink / 同名差し替えは可能。
  headless run 終了から dialog 起動までの窓（`src/cli/handoff.rs:290-298`:
  save_plan :290 → apply_prompt で plan パスを prompt に埋め込み :291-296 →
  advisor.direct で dialog 起動 :298）で plan 差し替えが成立しうる。
  apply_prompt の untrusted 前付け（`src/core/optimize.rs:806-807`）・
  項別 y/n（:810-811）・backup 手順（:820-829）は差し替えの検出自体はしない
- 同根の問題: `ensure_owner_only`（`src/store.rs:499-536`）は既存ファイルの
  chmod 失敗（= 他人所有）を `:524-525` で黙認し、攻撃者所有 0644 ストアに
  transcript 由来データが書き込まれて world-readable のまま残る
- プロジェクト側の前提表明: `src/store.rs:492-498`「nobody deliberately shares
  this store」。ただし前提が破られたとき黙って続行するのは前提表明と矛盾する。
  本 PBI の主眼は「黙って続行しない」ことに置く

## BDD受け入れシナリオ

```gherkin
Scenario: group/other 書き込み可能な既存 plans ディレクトリで警告が出る
  Given group/other 書き込み可能な 0777 の plans ディレクトリが既に存在する
  When --apply 付きの headless run が plan を保存する
  Then stderr に既存ディレクトリのモード・所有者異常を示す警告が 1 行出る
  And plan の保存と dialog 起動は従来どおり行われる

Scenario: 他人所有らしき既存ストアで警告が出る
  Given group/other readable な 0644 の既存 store ファイルがある
  When analyze が store を read-write で開く
  Then stderr に所有者・モード異常を示す警告が出る
  And analyze は完了する

Scenario: デフォルト環境の挙動は 1 バイトも変わらない
  Given 存在しない、または所有者 0700 のユーザ状態ディレクトリがある
  When 修正前と修正後のバイナリで同一入力の run を実行する
  Then stdout/stderr がバイト同一であり警告は出ない
```

## 受け入れ基準
- [x] `create_private_dir_all` の 2 箇所（`src/cli/deliver.rs:371-382` と
  `src/store.rs:472-483`）で、既存ディレクトリのモード・所有者異常を検出し
  stderr に警告する（検出条件: group/other ビットが立っている、または chmod
  0700 が拒否される — 最終条件は実装時に確定）
- [x] store の read-write open 経路（`src/store.rs:628-634`）で既存 store
  ファイルの同一検出を行い、`ensure_owner_only` が `:524-525` で黙認している
  PermissionDenied を警告付きにする
- [x] デフォルト環境（単一ユーザ home 配下、新規作成または正常な 0700/0600）では
  stdout/stderr が修正前とバイト同一である
- [x] 異常検出時も処理は継続する（stderr 警告、最低限）。明示的失敗にするかの
  設計判断は実装時に行い、docs/specs へ記録する
- [x] plan 差し替え窓の緩和として、save から dialog 読み取り間の所有者/mtime
  再検証を入れるか、過剰と判断して警告化・見送りにするかを実装時に判断し記録する
- [x] 同一ユーザで再現可能な自動テストがある（chmod で group/other writable に
  したディレクトリ・0644 にした store ファイルへの警告を pin）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）
- 単体: 検出関数の境界テスト。0777 / 0750 / 0700 の既存ディレクトリ、0644 /
  0600 の既存ファイルを入力に、「group/other ビット検出」と「chmod 拒否検出」
  を別々に固定する。新規作成・正常モードでは警告が 0 回であることを等価性の
  証人にする
- 単体: `ensure_owner_only` の PermissionDenied 経路。chmod 失敗をモックできない
  ため、検出判定を純粋関数（mode / metadata → 判定）に切り出し、判定部を
  直接テストする。core の純粋性方針に合わせ、I/O から判定を分離する
- 統合: 0777 の plans ディレクトリを仕込んだ headless run が stderr 警告を
  出しつつ完了すること。fixture は合成のみ（実 transcript・実 config を
  持ち込まない。session-data-privacy 規約）
- E2E: デフォルト環境での正常系 golden の diff が空であること。
  警告は異常時のみ stderr に出るため、正常系 golden には現れない

## 実装アプローチ
1. 検出ヘルパーを 1 つに集約する: 「既存パスの metadata を読み、
   group/other ビットの存在・chmod 可能性・所有者 uid の異常を判定する」
   純粋な判定関数 + I/O を担当する薄いラッパー。cfg(unix) でガードする
2. `src/cli/deliver.rs:371-382` と `src/store.rs:472-483` の両方の
   `create_private_dir_all` から判定を呼ぶ。2 箇所に同型実装が存在するのは
   PBI 07 の統合残（ボード記録）だが、本 PBI では統合そのものはしない —
   両方に同じ検出を足すことに留め、重複解消は別途
3. `src/store.rs:628-634` の read-write open 経路で、`ensure_owner_only` の
   PermissionDenied 黙認（:524-525）を「警告付き黙認」に変える。続行か失敗かの
   設計判断は実装時に確定させる
4. 警告出力は既存の `Style::stderr().warn` 経路（例: `src/cli/handoff.rs:288`）
   に倣い、stderr 専用とする。stdout のバイト同一性契約を壊さない
5. plan 差し替え窓（`src/cli/handoff.rs:290-298`）の再検証は、save 直後と
   dialog 起動直前の所有者/mtime 比較として試作し、過剰なら警告化または
   見送り。判断と根拠を docs/specs に記録する（spec-sync 規約）

## 見積もり
0.5日。検出ヘルパー 1 個とその呼び出し 2-3 箇所、警告文言、テストが実質の
作業。設計判断（警告 vs 失敗、再検証の要否）が思考の大半を占める

## 技術的考慮事項
- デフォルト環境の挙動不変（正常系 golden）がロールバック不要の条件。
  「存在しない / 正常 0700 / 正常 0600」では警告を出さず、1 バイトも
  出力を変えない
- stderr 警告の追加は e2e_golden に影響しない（警告は異常時のみ）。
  ただし警告を stdout に出すと golden が崩れるため stderr 専用とする
- 所有者検証は Windows で異なる。既存の unix 限定箇所
  （`src/cli/deliver.rs:372-379` / `src/store.rs:473-480` / `:500-532`）は
  いずれも `cfg(unix)` ガード済みで、Windows では検証なしの現行挙動を維持する
- 「chmod 可能性」を検出に使う場合、同一ユーザ所有の 0644 ファイルは
  縮退に成功するため chmod 失敗検出では警告が出ない。モード異常
  （group/other ビット）検出と併用して初めて同一ユーザで再現可能になる

## 実装者向け注記

### 現状コードの確認
以下は 2026-11-21 時点の現行行番号（実読取で確認済み）:
- `src/cli/deliver.rs:363-382` — `create_private_dir_all`。doc :367 が
  「Directories that already exist keep theirs」と明言し、既存ディレクトリの
  所有者・モードは検証しない
- `src/store.rs:469-483` — store 側の同名プライベートコピー。doc :471 に同文言。
  `src/store.rs:628-634` の `OpenMode::ReadWrite` 経路で親ディレクトリ作成
  （:631）→ `ensure_owner_only`（:634）の順に呼ばれる
- `src/store.rs:499-536` — `ensure_owner_only`。既存ファイルの chmod 失敗
  （PermissionDenied = 他人所有）を :524-525 で黙認。:492-498 の doc が
  「nobody deliberately shares this store」と前提を表明し、「refusing to run
  over it would break a store legitimately created under another account」を
  理由に挙げている
- `src/cli/handoff.rs:131-136` — `save_plan`。`create_private_dir_all` +
  `write_private_file` で再検証なし
- `src/session_env.rs:311-327` — `state_base`。絶対値の `$XDG_STATE_HOME` を
  何でも受理する（:319-320）。共有ディレクトリを指していても無検査
- `src/cli/handoff.rs:290-298` — `complete_handoff` 内、save_plan から
  advisor.direct（dialog 起動）までの窓
- `src/core/optimize.rs:806-807` — plan を untrusted と明示する前付け。
  :810-811 項別 y/n、:820-829 backup 手順。いずれも plan 差し替えの検出はしない

### 実装手順
1. 判定を純粋関数として切り出す（metadata 的情報 → 異常判定）。赤テストから:
   0777 ディレクトリ / 0644 ファイル / 正常 0700 / 正常 0600 の 4 折れ点
2. `src/cli/deliver.rs` の `create_private_dir_all` に検出を組み込み、
   stderr 警告を 1 行出す。既存テスト（同ファイルの `#[cfg(test)]`）が
   正常系等価性の証人になる
3. `src/store.rs` 側の `create_private_dir_only` コピーと `ensure_owner_only`
   に同一検出を組み込む。:524-525 の黙認箇所に警告を足す
4. 統合テスト: 0777 plans ディレクトリ + headless run。E2E golden で
   正常系のバイト同一性を確認
5. plan 差し替え窓の再検証を試作し、要否を判断。判断を docs/specs に記録
   （cli.md の handoff セクション + storage.md の permission セクション相当）
6. 全ゲート: cargo fmt --check / cargo clippy --all-targets --all-features
   -- -D warnings / cargo test

### 落とし穴
- 所有者検証は Windows で異なる（`std::os::unix` の限定箇所を確認）。
  `cfg(unix)` ガードを維持し、Windows では現行挙動（検証なし）を保つ
- テストで実際に他人所有ディレクトリを作るのは root が必要。実行可能な
  pin は「chmod で group/other writable にしたディレクトリへの警告」
  （同一ユーザで再現可能な検出）を設計すること。所有者 uid 比較に pin し
  すぎると同一ユーザでは検証不能になる
- 警告か失敗かを先送りしたまま「デフォルト続行」にすると「黙って続行」が
  残る。最低限 stderr 警告は必須とし、失敗化はオプトインの設計判断として
  docs/specs に記録する
- `src/store.rs:492-498` の既存 doc は「他人所有ストアを再モードしない」
  理由として正当なユースケース（別アカウントで作られたストア）を挙げている。
  警告はこの正当系も拾う — それでよい（前提表明の更新とセットで doc を
  直すこと）。失敗化する場合はこの正当系との衝突を解決してからにする
- store.rs は巨大（4600 行超）で同一日の他 PBI と競合しやすい。変更は
  :469-536 と :628-634 近傍に留める

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test がすべて green
- [x] デフォルト環境での正常系出力のバイト同一性を確認済み（修正前後の diff 空）
- [x] 警告/失敗の設計判断と plan 差し替え窓の再検証の要否判断が
  docs/specs に記録済み（spec-sync 規約）
- [x] `src/store.rs:492-498` の前提表明 doc が新挙動と整合するよう更新済み
- [x] コードレビュー完了（spec 準拠 + コード品質）
