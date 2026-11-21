# PBI: 端末出力の sanitize 境界を完全化する（agent 名・C1・bidi・CR）

## ユーザーストーリー
doctor の「WHAT TO FIX FIRST」や subagent 内訳を読んで直す場所を決める saniwalens
ユーザーとして、transcript 由来の agent 名・プロジェクト名・セッションの作業
ディレクトリがどんな制御文字や不可視文字を含んでいても、端末にそのまま流れない
状態がほしい。なぜなら現状は agent 名が sanitize を一切通らず ESC/CSI/OSC すら
素通しで、sanitize_cell 自体も C1・CR・bidi を素通しするため、doctor の prose は
「すべての transcript 由来値が sanitized 済み」という契約コメントの直反例を抱え、
出力の読み取りを妨害され得るからだ。

## ビジネス価値
- 順位 01 / 本ラウンド 13 件中。敵対的レビュー（反証 V3）で 4 指摘すべてが
  SURVIVES した最上位の 1 本に統合した。
- doctor の契約コメント（src/cli/views/doctor.rs:91-94）は「ESC, C1, or BEL を
  端末に運ばない」と宣言するが、実装はこれを満たさない。ドキュメントと実装の
  乖離を解消し、端末出力の完全性という invariant を全経路で回復する。
- bidi / ゼロ幅文字の実害は表示偽装と scope コピー乖離まで（saniwalens 自身は
  文字列を実行しない — `src/core/scope.rs:19-40` は opaque 文字列比較）だが、
  出力はユーザーの行動（scope のコピーや目視トリアージ）を直接導くため、
  偽装されないことが信頼の前提になる。

## BDD受け入れシナリオ

```gherkin
Scenario: agent 名に制御列を含むセッションでも doctor の subagent 行は安全に読める
  Given subagent 実行の agent 名に端末制御列を含む transcript を記録したセッションが 1 つある
  When doctor を実行する
  Then subagent の内訳の行に端末が解釈する制御バイトが含まれない
  And agent 名は usage 画面と同じ文字として表示される

Scenario: C1・bidi・CR を含む名前やパスは doctor のどの行にも流れない
  Given プロジェクト名・セッションの作業ディレクトリに C1 制御文字・双方向オーバーライド・CR を含む transcript がある
  When doctor を実行する
  Then stdout に行頭を偽装する制御文字が含まれない
  And 双方向オーバーライドやゼロ幅の不可視文字が含まれない

Scenario: 正常系の出力はバイト同一である
  Given 制御文字を含まない通常の transcript
  When この修正の前後で doctor を実行する
  Then stdout はバイト同一である
```

## 受け入れ基準
- [x] `sanitize_cell` が Cc 全域を除去する（`\n` / `\t` は保持する方針を doc
  comment に明示。これにより `\r` も prose に届かなくなる）
- [x] `sanitize_cell` が Cf の bidi（U+202A-202E）・ゼロ幅（U+200B-200F,
  U+FEFF）を除去する
- [x] doctor の subagent 内訳行（`doctor_agent_lines`）の agent 名が
  `sanitize_cell` を通る
- [x] C1 / bidi / CR を fixture に投入した E2E が、`is_control` ベースの assert
  を有効化した形で落ちない（`\r` のホワイトリスト除去を含む）
- [x] 正常系（通常のパス・名前 — Cc/Cf は含まれない）の出力はバイト同一である
  （証人: 全 golden）
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green

## テスト戦略（t_wadaスタイル）

### E2Eテスト
- `tests/doctor_prose_sanitized.rs` の fixture（`hostile_path` :22-25 と
  `SessionMeta` :108-112）に C1（UTF-8 エンコードの 0xC2 0x80-0x9F）・bidi /
  ゼロ幅・CR を投入する。assert（:50-61）は `is_control` ベースのまま
  ホワイトリストから `'\r'` を外して有効化する — C1 と CR は `is_control` が
  検出するため既存 assert が効く
- bidi / ゼロ幅は `is_control()` が Cc のみ対象のため検出できない。不在 assert
  （出力にこれらの符号位置が現れないこと）を明示的に置く
- 正常系のバイト同一性は既存 golden 群が証人

### 統合テスト
- hostile な agent 名を含む findings を doctor の prose 生成に渡し、subagent
  内訳行に制御バイトが混入しないことを行単位で assert する
- `escape_markdown_cell`（src/cli/report.rs:234-239）との役割分担を固定する:
  sanitize 強化後も markdown 経路の `<br>` 折り畳み（`\n` 保持が前提）が
  機能し続けること

### 単体テスト
- `sanitize_cell` の境界テスト: C1 の各バイト（U+0080-009F、端末で 8-bit CSI と
  解釈されうる U+009B を含む）、bidi / ゼロ幅の各符号位置、`\r`、`\n` / `\t`
  の保持、日本語など通常のマルチバイト文字の保持。Red → green

## 実装アプローチ（Outside-In, Red-Green-Refactor）
1. **Outside-In — E2E を先に Red にする**: fixture に C1 / CR / agent 名の制御列
   を投入して現状の素通しを可視化する（C1 / CR は `is_control` assert で落ち、
   agent 名の ESC は sanitize 非経由で落ちる）。bidi / ゼロ幅は不在 assert を
   追加してから投入する
2. **Green — sanitizer を 1 箇所強化**: `sanitize_cell`（src/cli/report.rs:273-334）
   を byte スキャンから multi-byte 意識の実装に強化し、Cc 全域（`\n` / `\t` 除く）
   と Cf の bidi / ゼロ幅を落とす。既存の ESC/CSI/OSC/BEL 処理は維持する
3. **Green — 呼び出し側を 1 箇所直す**: `doctor_agent_lines`
   （src/cli/views/doctor.rs:410-417）の `a.label()` を `sanitize_cell` 経由にする
4. **Refactor — 契約コメントを実装に合わせる**: report.rs:261-272 の
   sanitize_cell doc と doctor.rs:91-94 の契約コメントを、実際の語彙（Cc / Cf、
   `\n` / `\t` 保持方針）で書き直す
5. 全 gate と golden によるバイト同一性確認

## 見積もり
1日（敵対的レビューの見積もりと同一根拠）

## 技術的考慮事項

### 依存関係
- 変更は sanitizer 1 箇所（report.rs）と呼び出し側 1 箇所（doctor.rs の
  `doctor_agent_lines`）のみ。store・adapter・他 view に触れない
- usage view は同じ agent 名を TableSpec cell（src/cli/views/usage.rs:69-82）で
  `render()` → `sanitize_cell` 経由に載せており、sanitizer 強化の恩恵を自動で
  受ける。doctor だけが生補間していた非対称がここで解消される
- ロールバック不要: 挙動は防御の追加のみで、正常系はバイト同一

### テスタビリティ
- `sanitize_cell` は純粋関数で単体テスト容易
- E2E は `tests/doctor_prose_sanitized.rs` の既存ハーネス（合成 home / store、
  実 transcript なし — プライバシー規則準拠）を流用する

### 非機能要件
- スキャンは 1 パス O(n) を維持し、性能劣化なし
- 正常系出力はバイト同一（Cc / Cf は通常のパス・名前に含まれない）
- fixture は合成データのみ。harvest した transcript や実 config は持ち込まない

## 実装者向け注記

### 現状コードの確認
```bash
rg -n "doctor_agent_lines|a\.label\(\)" src/cli/views/doctor.rs || true
rg -n "0x1b|0x07|is_control|ASCII" src/cli/report.rs tests/doctor_prose_sanitized.rs || true
rg -n "sanitize_cell" src/cli/views/doctor.rs src/cli/report.rs || true
```

4 指摘の現位置（検証済み）:
1. **[高] agent 名が sanitize 非経由**: `doctor_agent_lines` が
   `a.label()`（`src/core/optimize.rs:104-106` — `subagent_runs.agent` の生文字列。
   由来は sidecar の agentType または opencode の state.input.subagent_type）を
   format 補間し（src/cli/views/doctor.rs:410-417）、戻り値を doctor.rs:267-273 が
   生 `println!` する。`sanitize_cell` を一切通らず ESC/CSI/OSC 全語彙が通る。
   同じ値を usage view は render() 経由で保護している非対称。
   doctor.rs:91-94 の契約の直反例
2. **[中] C1 素通し**: `sanitize_cell`（src/cli/report.rs:273-334）は 0x1b と
   0x07 のみ除去し、default arm（:325-328）で UTF-8 エンコードの生 C1
   （0xC2 0x80-0xC2 0x9F — U+009B は端末で 8-bit CSI と解釈されうる）を素通し
   させる。doctor.rs:91-94 は「ESC, C1, or BEL」を契約する → コメントが偽
3. **[中] CR 素通し**: `sanitize_cell` は CR を除去しない（:322-328）。
   無正規化の `session_cwd`（src/adapter/transcript.rs:720-730）→
   `normalize_root`（src/adapter/claude.rs:140-160、.wt 折り畳みのみ）→
   sessions.root → doctor の root / project 表示（doctor.rs:95-101 / 113-117,
   140-143, 296-299）で行頭偽装が可能。work event の path は `clean_truncate` の
   whitespace collapse で保護済みだが、root 経路と既存 store 行が生きる
4. **[低] bidi / ゼロ幅素通し**: bidi（U+202A-202E）・ゼロ幅（U+200B-200F,
   U+FEFF）は Cf で `is_control()` の対象外。実害は表示偽装と scope コピー乖離
   まで（`src/core/scope.rs:19-40` は opaque 文字列比較、実行なし）

E2E の穴: `tests/doctor_prose_sanitized.rs` の fixture（:22-25, :108-112）は
ESC/CSI/OSC/BEL のみで C1 / bidi を含まず、:54 は `'\r'` を許可ホワイトリスト
入り（:47-49 の doc「No control byte beyond line structure」と矛盾）。

### 実装手順
1. E2E を Red にする: `hostile_path` と `SessionMeta` に C1 / bidi / CR を投入、
   hostile agent 名のセッション fixture を新設、assert の `'\r'` ホワイトリスト
   除去 + bidi / ゼロ幅の不在 assert を追加 → `cargo test --test
   doctor_prose_sanitized` で現状が落ちることを確認
2. `sanitize_cell` を強化: Cc 全域除去（`\n` / `\t` は保持）、Cf の bidi / ゼロ幅
   除去。byte 直接走査の前提（multi-byte は触らない）が崩れるので、multi-byte
   列を壊さない実装に替える。:331-333 の `from_utf8` expect は
   「ASCII bytes only」前提で書かれているため更新が必要
3. `doctor_agent_lines` の `a.label()` を `sanitize_cell` 経由にする
4. doc comment を契約に合わせ更新（report.rs:261-272 / doctor.rs:91-94、
   `\n` / `\t` 保持方針を明示）
5. gate: cargo fmt --check / cargo clippy --all-targets --all-features --
   -D warnings / cargo test。golden で正常系バイト同一を確認

### 落とし穴
- `escape_markdown_cell`（src/cli/report.rs:234-239）は markdown 経路のみの別
  実装 — prose 側の強化と混同しない
- `sanitize_cell` を強化すると全 view の table cell も影響する。バイト同一の
  証人は全 golden（正常系は Cc / Cf を含まないため変化しないはず）
- C1 を除去しても、既存 store 行を再分析しない限り旧データからの prose は
  変わらない。この PBI は出力側の防御で、store 再分析・ingest 側正規化はしない
- `is_control()` は Cc のみ対象 — bidi / ゼロ幅の新 fixture は既存 assert では
  検出不能。不在 assert（またはバイト同一性）で検出すること

## Definition of Done
- [x] 全 BDD シナリオが自動テストとして実装されパスする
- [x] 正常系出力のバイト同一性を golden で確認
- [x] cargo fmt --check / cargo clippy --all-targets --all-features -- -D warnings /
  cargo test が green
- [x] コードレビュー完了（バッチ統合レビュー: 全 diff・spec 同期・干渉を目視確認）
- [x] ドキュメント更新済み（sanitize_cell / doctor の契約コメント、spec-sync
  該当があれば）
