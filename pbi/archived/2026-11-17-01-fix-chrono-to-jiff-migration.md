# PBI: chrono を jiff に置き換える

## ユーザーストーリー
メンテナとして、unmaintained になった chrono への依存をやめたい、なぜなら
RUSTSEC-2026-0543 により `cargo deny` が失敗し、全 PR の CI が赤になるから

## ビジネス価値
- CI の依存関係ゲート（cargo-deny）を green に戻し、以降の PR を流せるようにする
- 将来の chrono 由来のセキュリティ issue を恒久的に回避する
- 測定方法: `cargo deny check` が exit 0、`cargo test` 全緑、出力のバイト同一性維持

## フェーズ0: 既実装確認（済み）
- `grep -rn "chrono" src/ tests/ Cargo.toml` を実施。jiff / time への移行は存在しない
- chrono 使用箇所: `src/session.rs`、`src/core/bucket.rs`、`src/core/period.rs`、
  `src/store.rs`、`src/adapter/transcript.rs`、`src/adapter/harness.rs`、
  `src/cli/cleanup.rs`、`src/cli/window.rs`、`Cargo.toml`
- 既存コマンド一覧への影響なし（依存差し替えのみ、CLI の振る舞いは不変のはず）

## BDD受け入れシナリオ

```gherkin
Scenario: deny ゲートが通る
  Given RUSTSEC-2026-0543 が公開されている状態
  When cargo deny check を実行する
  Then exit 0 になる（chrono への依存が消えている）

Scenario: 時刻の出力が変わらない
  Given 移行前のバイナリと移行後のバイナリ
  When 同じ transcripts・config に対して doctor / waste / usage 等を実行する
  Then stdout がバイト同一である（RFC3339 表記・JST バケット境界・--since/--last 境界を含む）

Scenario: ローカル時刻の表示が変わらない
  Given mtime ベースの表示（cleanup の日時表示）がある状態
  When cleanup --list を実行する
  Then 移行前と同じ文字列が出る（タイムゾーン解決を含む）
```

## 受け入れ基準
- [x] `cargo deny check` が exit 0
- [x] `cargo test --locked` 全緑（既存テストが時刻フォーマットのピン留めになっている）
- [x] `cargo fmt --check` と `cargo clippy --all-targets --all-features -- -D warnings` が通る
- [x] `Cargo.lock` に chrono が残っていない
- [x] 代表コマンドの新旧出力 diff が空（少なくとも doctor / waste / usage / failures）

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- 既存の `cargo test` スイート全体（時刻まわりの golden が多数ある。これが Red/Green の判定線）
- `cargo deny check`（現在 Red のゲート。これが Green になることが完了条件）

### 統合テスト
- 新旧バイナリの出力 diff（同一入力に対する doctor / waste / usage / failures / prompts / stuck）

### 単体テスト
- RFC3339 パース（オフセット付き・Z 付き・小数秒）の等価性
- JST バケット境界（period.rs の既存テストがカバー。追加は等価性が怪しい箇所のみ）
- `Local` 解決（cleanup の日時表示）

## 実装アプローチ
- **Outside-In**: まず `cargo deny check` の失敗を確認（済み・Red）→ 移行 → Green → リファクタ
- **Red-Green-Refactor**: 既存テストを壊さない最小差分で進め、テストが赤くなったら即座に戻す
- ファイル横断だが依存差し替え1件の縦スライスなので分割しない

## 見積もり
5pt（暫定・要チーム見積もり）。時刻意味論の差異（丸め・`Z` 表記・Local 解決）がテストで顕在化した場合は増える

## 技術的考慮事項
- 依存関係: chrono 0.4.45（`default-features = false, clock`）→ jiff（features は必要最小限。`std` + `tzdb-bundle-platform` 等の要否は実装時に決める）
- テスタビリティ: 既存 golden テストが等価性の証人になる。新規テストは差異が出た箇所にだけ足す
- 非機能要件: 出力のバイト同一性（このリポジトリは `--help` 出力やレポートの安定性を契約にしている）

## 実装者向け注記

### 現状コードの確認（済み）
```bash
grep -rn "chrono::" src/ | grep -v "^.*://" | grep -v "chrono can express\|chrono upgrade\|chrono format"
```

### API 対応の目安（要テストで裏取り）
- `chrono::Utc::now()` → `jiff::Timestamp::now()`
- `DateTime::parse_from_rfc3339(s)` → `s.parse::<jiff::Timestamp>()`（どちらもオフセット必須）
- `DateTime::from_timestamp(sec, 0)` → `jiff::Timestamp::from_second(sec)`
- `DateTime::from_timestamp_millis(ms)` → `jiff::Timestamp::from_millisecond(ms)`
- `FixedOffset` → `jiff::tz::Offset` / `TimeZone::fixed`
- `NaiveDate` / `NaiveDateTime` → `jiff::civil::Date` / `jiff::civil::DateTime`
- `DateTime::<Local>::from(...)` → システムタイムゾーンの `jiff::Zoned`
- `.to_rfc3339_opts(SecondsFormat::Secs, true)` → 秒精度＋`Z` の RFC3339（jiff の Display と1文字ずつ突き合わせること）
- `period.rs` のテストは chrono の書式・範囲にピン留めされている。差分が出たら jiff 側を寄せるか、意図的変更として spec 同期する（`.claude/rules/spec-sync.md`）

### 落とし穴
- jiff の `Timestamp` Display は既定で RFC3339 互換だが、小数秒の有無・`Z` 表記を盲信しない。必ず既存テストで確認する
- `Local`（cleanup.rs）は実行環境の TZ に依存する。CI（ubuntu/macos/windows）で Green になること
- `Cargo.lock` の更新を忘れない（`--locked` で CI が落ちる）
- spec に時刻書式の記述があれば `.claude/rules/spec-sync.md` に従い同梱更新する

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] `cargo deny check` / `cargo test --locked` / `just check` がすべて Green
- [x] 新旧バイナリの代表コマンド出力 diff が空
- [x] コードレビュー完了（PR #41 マージ 2026-11-19 にて確認）
- [x] ドキュメント更新済み（spec-sync 該当があれば）
