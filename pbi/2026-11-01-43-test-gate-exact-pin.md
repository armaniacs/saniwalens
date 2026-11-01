# PBI: prose gate の否定 assert をパス成分の exact pin にする

## ユーザーストーリー
保守担当者として、prose reference gate のスキャン対象・除外判定がチェックアウト先の
パス文字列に依存せず、パス成分で exact に判定されてほしい、なぜなら絶対パスの部分一致は
`docs/privacy.md` を履歴と誤認し、Windows の `\` 区切りでは除外が空振りし、逆に
チェックアウト先が `pbi` を含むだけで無変更の gate が赤になるからだ。

## 優先度
- 順位: 1 / 全候補数 10
- RICEスコア: 24.0（Reach=2 / Impact=2 / Confidence=3 / Effort=0.5日）
- 根拠: 機構が probe で確定し、修正がテストファイル 1 本に閉じる。将来の Windows
  テスト着地（PBI 02）の前提でもあり、誤検知で gate 自体が撤去されるリスクを先に閉じる。

## ビジネス価値
- gate の除外契約が「絶対パス文字列の部分一致」から「root 相対のパス成分」になり、
  checkout 先・OS・近似名に依存しなくなる。
- 測定方法: `docs/privacy.md`、`pbi-notes.md`、`docs/specs-history/x.md` を履歴と
  誤認しないこと。`docs/pr/x.md`、`pbi/x.md`、`CHANGELOG.md` を確実に除外すること。

## 背景（なぜなぜ分析 2026-11-01、30段階 x 3立場、file:line 実証）
- `tests/prose_reference_gate.rs:288-297` の否定 assert は `to_string_lossy()` した
  絶対パスに `contains("docs/pr")` / `contains("pbi")` / `contains("CHANGELOG")` を
  使う。肯定 assert（:278-287）も同じ弱い形。
- 帰結: (a) checkout 先が `pbi` や `docs/pr` を含むと無変更で赤（自壊）、
  (b) Windows の `\` 区切りで `docs/pr` と `docs/exploration` の 2 節が空振り、
  (c) `docs/specs-history` を履歴として許可し、`pbi-` 接頭辞の正当名を誤検知する。
- 根本原因: 除外の契約が「絶対パス文字列の部分一致」で書かれ、判定が「root 相対の
  パス成分」で書かれていないこと。ゲート本体（:37-75, :81-144）の欠陥ではない。

## BDD受け入れシナリオ

```gherkin
Scenario: checkout 先が pbi を含んでも current prose は通る
  Given リポジトリが /tmp/pbi-work/saniwalens にチェックアウトされている
  When prose reference gate を実行する
  Then scan 対象の全ファイルが通過し、履歴の誤認で赤にならない

Scenario: 近似名と履歴を成分で区別する
  Given docs/privacy.md、pbi-notes.md、docs/specs-history/x.md が存在する
  And docs/pr/x.md、pbi/x.md、docs/exploration/x.md、CHANGELOG.md が存在する
  When 除外判定を通す
  Then 前者は current prose として扱われ、後者だけが history として除外される

Scenario: 親ディレクトリ参照を履歴判定に使わない
  Given パスに ".." が含まれる
  When 除外判定を通す
  Then 成分比較で history と誤認せず、ParentDir を拒む pin が存在する
```

## 受け入れ基準
- [ ] `tests/prose_reference_gate.rs` に root 相対化と分類の純関数
  (`rel` / `is_current_prose` / `is_history` 相当) を追加する
- [ ] 肯定・否定の両 assert を `Path::starts_with` など成分比較へ差し替え、
  絶対パスへの `contains` を残さない
- [ ] `..` (ParentDir) を history と誤認しない pin を追加する
- [ ] 実ツリーの履歴ディレクトリ (`docs/pr`、`docs/exploration`、`pbi`) が存在し、
  scan されないことを pin する
- [ ] 除外リストを 1 つの定数に集約し、doc comment 3 箇所と
  `.claude/rules/spec-sync.md` の current prose 記述を同じ変更で更新する
- [ ] ゲート本体（span 抽出・fence 判定・resolves）は変更しない
- [ ] `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
  `cargo test --test prose_reference_gate` が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 実リポジトリの `scan_files` が 21 件前後の current prose を返し、履歴 0 件であること

### 統合テスト
- 既存 `every_occurrence_of_a_dead_anchor_is_listed` と `fenced_code_blocks_are_not_scanned`
  が不変で green

### 単体テスト
- `docs/privacy.md` / `pbi-notes.md` / `docs/specs-history/x.md` を history としない
- `docs/pr/x.md` / `pbi/x.md` / `docs/exploration/x.md` / `CHANGELOG.md` を history とする
- `..` を含むパスの扱い
- 合成 root（`/tmp/pbi-work`）で旧述語が赤・新述語が緑になること

## 実装アプローチ
- **Outside-In**: 誤検知・空振りの再現テストを先に書き、次に純関数を抽出して
  assert を差し替える。最後に doc と spec-sync の文言を合わせる。
- 実装は subagent-driven-development スキルで実施する。

## 見積もり
0.5 SP（E=0.5日。RICE 24.0）

## 技術的考慮事項
- 依存関係: なし。被依存: PBI 02（Windows test 着地前に exact 化が必要）
- テスタビリティ: 純関数を `file_misses` とは独立に単体テストできる形にする
- 非機能要件: プライバシー（合成パスのみ）、Windows 区切りは文字列シミュレーションで pin

## 実装者向け注記

### 現状コードの確認（2026-11-01 なぜなぜ分析で file:line 実証済み）
- `tests/prose_reference_gate.rs` の `scan_files` は絶対 `PathBuf` を返す。
  `the_scan_list_is_current_prose_only` は :274-297。
- 除外対象は `docs/pr`、`docs/exploration`、`pbi/`、`CHANGELOG`。
  `docs/specs/*.md`、`.claude/rules/*.md`、`README.md`、`AGENTS.md` だけが current prose。
- `Path::starts_with` は成分一致（`docs/privacy` は `docs/pr` に一致しない）。
  probe で `docs/specs-history` / `docs/privacy` / `pbi-x` がすべて false になることを確認済み。

### 実装手順
1. Red: 合成 root と近似名で旧 assert が誤判定するテストを書く
2. `rel(root, file)` と `is_history(rel)` / `is_current_prose(rel)` を追加
3. 肯定・否定 assert を差し替え、`ParentDir` と実ツリー履歴の pin を追加
4. 除外リストを定数化し、doc comment と spec-sync を更新
5. 検証（対象テスト -> fmt/clippy -> 最後にフルスイート）

### 落とし穴
- `canonicalize` は symlink を実体化して checkout 外を見るため使わない
- `strip_prefix(root)` の失敗は panic ではなく明示的な gate 失敗にする
- Windows の `\` は `Path` が吸収するが、文字列比較に戻さない
- 行番号は既にずれているため、PBI 36 の行番号ではなく関数名で参照する

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] 否定・肯定 assert がパス成分比較になっている
- [ ] 対象テスト、fmt、clippy、フルスイートが green
- [ ] コードレビュー完了（task review + 最終 review）
- [ ] spec-sync と doc comment 更新済み
