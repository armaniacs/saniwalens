# PBI: adapter の fuzz / property test を追加する

## ユーザーストーリー
開発者・メンテナとして、transcript / config adapter の防御的 deserialize を実証する property test または fuzz ターゲットがほしい。なぜなら「未知のフィールドや config key が analyze を壊さない」という spec の主張を、実際に壊れると失敗するテストで裏付け、ハーネス更新による前方互換性の後退を機械的に検知できるようにするからだ。

## 優先度
- 順位: 20 / 全28件
- RICEスコア: 2.7（Reach=2 / Impact=2 / Confidence=2 / Effort=3日）
- 根拠: 前方互換性の保証の実証。tool の価値は分析ロジックにあるため、同スコアの中では計測改善系の後ろに置く。依存関係はなし。

## BDD受け入れシナリオ

Scenario: 未知の要素を混ぜた合成入力でも解析が壊れない
  Given 未知のフィールド、必須項目の欠損、型の違いを混ぜた合成の transcript と config を用意する
  When  解析を実行する
  Then  エラーにならず、既知の部分だけが取り込まれる

Scenario: 未知の tool denial kind は本文テキストへのフォールバックで分類される
  Given 将来のリリースで増えた、tool が知らない種別名の tool denial を含む transcript を用意する
  When  解析を実行する
  Then  その拒否は種別名に頼らず、本文テキストへのフォールバックで分類される

## 受け入れ基準
- [ ] 未知フィールド・欠損・型違いを混ぜた合成入力で analyze がエラーにならず、既知部分だけ取り込むことを、property test または fuzz ターゲットが自動検証する
- [ ] 未知の tool denial kind が本文テキストへのフォールバックで分類されることをテストが裏付ける
- [ ] fuzz（cargo-fuzz）か property（proptest）の選定が discovery で行われ、選定理由が記録される
- [ ] 失敗した場合に、どの入力で壊れたかを再現できる形で失敗メッセージが出る
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: 破壊的入力（unknown、欠損、型違い）を混ぜた合成 transcript と config で analyze コマンドが非ゼロで落ちないこと
- 統合: 防御的に取り込んだ入力について、既知フィールドのみが store に反映され、未知部分が結果を汚染しないこと
- 単体: transcript / config の各 deserialize に対する property（任意の JSON 断片を与えて panic しない）、将来 kind のフォールバック分類、境界（空文字、巨大ペイロード、ネストした未知オブジェクト）

## 見積もり
3日（RICE の Effort と同一根拠）

## 実装者向け注記
- spec の主張の対象: docs/specs/architecture.md「Forward compatibility」節（130 行付近）。「The adapter deserializes defensively: it reads only the fields the tool needs, ignores unknown fields, tolerates missing optional ones. A new Claude Code field or config key must never break analyze」をテストで裏付けるのがこの PBI。
- 既存テストの例: src/adapter/transcript.rs 1103 行付近「A kind from a future release: the text still gets its say」。toolDenialKind に未知の種別が来ても本文テキストで分類される点テストを、property / fuzz に引き上げる。
- 選定は discovery で: fuzz（cargo-fuzz）は nightly と CI 整備が要る。property（proptest）は cargo test に載る。リポジトリの検証コマンド（cargo test）で回る形を優先し、判断と理由を注記に残すこと。
- 「エラーにならない」の意味を spec に合わせること: 黙って全体を捨てるのではなく、既知部分だけ取り込む。入力の壊れ方で解析結果が誤読されない範囲での防御。
- adapter にハーネス入力形式の知識を集約する原則は崩さない（.claude/rules/format-isolation.md）。fuzz ターゲットの置き場所（fuzz/ 配下か tests/ 配下か）も選定に含める。
- privacy: 生成する入力は合成のみ。実 transcript を種にしない。
- 検証コマンド: cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test。

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
