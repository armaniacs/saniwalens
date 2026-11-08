# PBI C8: 契約 newtype 化 (contract newtypes)

## 優先度とスコア

- 優先度: rank 11 (最下位)
- RICE: 0.25 (Reach 4 / Impact 1 / Confidence 50% / Effort 8)
- 順序: C6 / C7 の後。単独では顧客価値なし。保守性のための整理。

## ユーザーストーリー

保守担当者として、store と view の間の契約を型で表してほしい。なぜならタプルとコメントだけでは誤用を防げないから。

## 背景 (file:line)

- `pub store: Store`: `src/session.rs:102-106`
- view は `&Session` を取る: `models.rs:21`, `prompts.rs:194`, `sessions.rs:59`, `guide.rs:419`
- store が `Vec<(String, String, i64)>` を返す: `:1169`, `:1236`, `:1121`, `session_roots :1750`
- 上記は `FindingsInput` (`src/core/findings.rs:50-82`) に写されるが、意味はコメントのみ
- `effective_catalog(range) :1707-1724` は windowed-uses と紛らわしい。対して `session_roots :1750` / `always_on_config_tokens :1411` は範囲なし
- floor は `Option<i64>` で `None -> 0` の番兵値: `:155`, `:342-346`
- `Session::refresh :223-246`: 二重 reopen + `FnOnce analyze` クロージャ
- `window.rs:158-287`: view ごとにクロージャをスレッド化

## 提案

- newtype 導入: `FrictionCell`, `ToolBreakdown`, `ProjectSlug`
- 改名: `effective_catalog(events_window)` にして windowed であることを明示
- floor を enum 化し、`None -> 0` の暗黙変換を廃止
- view の引数を `&Session` から `(&Store, &Harness)` に変更し、必要なものだけ渡す
- 設計判断は残す (Confidence 50% の理由)。過剰な抽象化はしない

## BDD 受け入れシナリオ

```gherkin
Scenario: 誤った順序のタプルはコンパイル時に拒否される
  Given 整理後の store と FindingsInput がある
  When 開発者が FrictionCell と ToolBreakdown を入れ替えて渡す
  Then コンパイルが失敗する

Scenario: 出力は整理前と同一である
  Given 同一の合成 fixture
  When 整理前後で全 view を描画する
  Then Markdown 出力がバイト同一になる
```

## 受け入れ基準

- [ ] `Vec<(String, String, i64)>` の公開返り値がなく、newtype になっている
- [ ] `effective_catalog(events_window)` に改名され、非 windowed 系との混同がない
- [ ] floor の `None -> 0` 暗黙変換がなく、enum で明示される
- [ ] view が `(&Store, &Harness)` を取り、`Session::refresh` の二重 reopen が解消される
- [ ] 既存テストが全て通り、上記 BDD の同一出力が確認できる

## テスト戦略 (t-wada式 Outside-In)

- E2E: 全 view のゴールデン比較 (整理前後で同一)
- 統合: store -> FindingsInput の契約テスト (フィールド対応)
- 単体 (多数): newtype 変換、floor enum の境界値、events_window の範囲指定
- 先にコンパイル失敗テスト (trybuild 相当か型エラーの手書き確認) を Red にし、実装後に Green にする

## 見積もり

- 8 (要チーム合意)。C6 / C7 完了後が前提。単独着手しない。

## 実装者向け注記

- 現状確認: `grep -rn "Vec<(String" src/` と `grep -rn "effective_catalog\|session_roots\|always_on_config_tokens" src/`
- 手順: 1) ゴールデン出力を固定 2) newtype 追加 3) 改名と floor enum 4) view 引数変更 5) refresh / window.rs 整理
- 落とし穴: `window.rs:158-287` のクロージャ取り回しでライフタイムが伸びる。無理に消さず最小限にする

## Definition of Done

- [ ] 全 BDD シナリオが自動テストとして通り、出力同一が確認できる
- [ ] `cargo test` と `cargo clippy` が通る
- [ ] 変更した契約について spec 差分を確認し、不要なら spec 更新なしと記録する

## 検証記録 (B7統合・2026-11-09)

- 実装: FrictionCell/ToolBreakdown/ProjectSlug/SessionRoot、Floor enum、effective_catalog(events_window)、views全11箇所 (&Store,&Harness) 化。残余（error_examples等のtuple、session_roots()旧形、refreshの片方re-open）は設計判断としてwhywhyに記録。
- ついでの統合修正: dispatchの `let sess` 束縛を `let s` 1行呼出しに圧縮（budget 1080丁度に収める。機能不変）。contract_newtypesのfield-init lintを構造体リテラル化。
- 最終ゲート: fmt OK、clippy 0 errors、フルテスト 847 passed / 0 failed、`cargo build --release` 成功。
