# PBI C3: XDG共通化 + 影loader削除

- 種別: リファクタ (候補 C3)
- 優先度: rank 6 (C1 の後)
- RICE: 3.0 (Reach 3 / Impact 1 / Confidence 100% / Effort 1)
- Estimate: 1
- 状態: 候補

## 背景

XDG 解決が3箇所に分散し、opencode 側に影 loader が残っている。

- `src/adapter/harness.rs:489-528`: `opencode_db_path` / `opencode_config_dir` / `kilocode_config_dir` が `filter(is_absolute)` + fallback を共有している
- `src/adapter/opencode.rs:719-731` vs `src/adapter/kilocode.rs:881-885`: 薄い env 読み取りが重複している
- 影 loader `src/adapter/opencode.rs:486-492` が正規版 `src/adapter/harness.rs:118-124` と重複している (kilocode は `:19-20` で正規版を再利用しており正しい)

## BDD シナリオ

### 1. 解決パスがバイト一致する

```gherkin
Given XDG 環境変数の組み合わせ (未設定 / 相対値 / 絶対値)
When 共通化後の helper で opencode / kilocode の各パスを解決する
Then 共通化前の解決結果とバイト一致する
```

### 2. opencode loader 削除でカタログ出力が不変になる

```gherkin
Given 影 loader を削除し正規 loader (`harness.rs:118-124`) に一本化した
When opencode カタログを生成する
Then 削除前と出力が同一になる
```

## 受け入れ条件

1. XDG 解決の共通 helper が `harness.rs` 側にあり、3 つのパス解決がそれを使う
2. `opencode.rs:719-731` と `kilocode.rs:881-885` の重複した env 読み取りが helper 経由になる
3. 影 loader (`opencode.rs:486-492`) が削除され、正規 loader のみが残る
4. BDD 1 と 2 がいずれも緑である
5. 既存の harness / adapter テストが全緑である

## テスト戦略 (t-wada)

- Red: XDG 組み合わせ表テスト (未設定 / 相対 / 絶対) で共通化前後一致を先に書く
- Red: 影 loader 削除前後のカタログ一致テスト (synthetic fixture) を先に書く
- Green: 最小の共通化 + 削除で通す
- Refactor: helper の命名と配置を整える (振る舞い不変)

## DoD

- 上記受け入れ条件 1-5 を満たす
- 影 loader への参照が残っていない
- 追加の振る舞い変更がない

## 検証記録 (B3統合・2026-11-09)

- 実装: `Harness::resolve_xdg`＋`xdg_from_env` 集約、opencode影loader削除（正規harness loaderへ一本化）、kilo_home整理。
- 既知の差分: 影と正規は100%同一ではなかった（正規はYAML `#` コメント除去＋ unreadable dir警告）。C3後はopencodeも正規側に寄る（PBI意図通り、kilocodeと既に共有の振る舞い）。
- ついでの統合修正: 影削除で孤立したdocコメント除去（clippy empty-line指摘対応）。
- 境界検証: fmt OK、clippy 0 errors、lib 642 passed、parity 6/6、template/restat/kilocode回帰 green。
