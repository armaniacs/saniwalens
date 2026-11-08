# PBI: opencode の per-run コストを計測する

## ユーザーストーリー
opencode を使う saniwalens ユーザーとして、run の呼び出し単位（callID）でコストを events に反映する機能がほしい、なぜなら opencode のコスト解像度が上がり、重い run を見分けられるからだ。

## 優先度
- 順位: 16 / 全28件
- RICEスコア: 3.0（Reach=1 / Impact=2 / Confidence=3 / Effort=2日）
- 根拠: opencode 単一ハーネス限定の改善で影響パスが狭いため 3.0。callID を使う結び付けの道筋が spec で確定しているため確信度は高い。依存: なし（opencode アダプタ内で完結）。

## BDD受け入れシナリオ
Scenario: run 単位のコストが使用履歴に載る
  Given opencode の transcript に、コストの分かる run（subagent の実行）が含まれている
  When  ユーザーが opencode の使用状況を計測する
  Then  run 単位のコストが使用履歴に反映され、どの run が重いかが分かる
  And   セッション全体の合計は従来どおり整合し、二重計上されない

Scenario: 結び付けられない行は従来の取り扱いのまま残る
  Given opencode の transcript に、コストを run に結び付けられない行が存在する
  When  ユーザーが opencode の使用状況を計測する
  Then  その行は従来の取り扱い（run 単位で分かれない集計）のまま残る
  And   行が欠落や破棄にならず、セッション合計の整合が保たれる

## 受け入れ基準
- [x] opencode の per-run コストが callID での結び付けにより events に反映される（実機のmetadata.sessionId結合を発見・実装。subagent_runs行＋span吸収を確認）
- [x] callID で結び付けられない行は従来の取り扱いのまま残る（sessionId無し/不明時はrunを作らない。単体テストで固定）
- [x] 結び付けは自セッション内に限る（runsは親セッション毎に構築し自spanにのみ照合。重複callIDでも誤帰属しない）
- [x] claude / kilocode の解析出力は一切変わらない（opencode.rsのみ。共通coreの変更なし）
- [x] docs/specs/session-format-opencode.md の Subagent linkage 節を更新する（実装済みとして全面改訂。nestingは対象外と明記）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: opencode の合成 transcript で、run 単位のコストが計測結果に載ること、結び付けられない行が従来扱いのまま残ることをユーザーシナリオとして確認する
- 統合: アダプタの出力が store へ正しく入ること（run 単位の行とセッション合計の整合、二重計上の無さ）を確認する
- 単体: callID の一致・不一致・重複・欠落の境界、結び付けられない行の従来扱い、他ハーネス入力が変わらないことの確認をテストする

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- docs/specs/session-format-opencode.md の Subagent linkage 節（128 行目から 133 行目付近）に「A future change that finds per-run costs joins on callID / metadata.sessionId without disturbing this mapping」とある。spec 更新必須（この将来変更を確定した取り扱いに更新し、節を同じ変更内で書き換える）
- 現状コードの確認（2026-10-31 実施済み）: src/adapter/opencode.rs は callID を既に読んでいる（577 行目付近）。task spawn は親側で exact、per-run のコスト割り当ては v1 では範囲外と明記されていた。per-run の格納先（src/store.rs 59 行目付近の subagent_runs の列か、events の列か）は spec 更新とセットで決める
- ハーネス別知識はアダプタに集約する（.claude/rules/format-isolation.md）。core・store の共通挙動と他ハーネス（claude / kilocode）は触らない
- opencode の format は上流で変わる前提。合成 fixture でテストし、実 transcript はコミットしない（プライバシーはハード制約）
- TDD: 単体（callID 結び付けの境界）→ 統合（store への反映と合計の整合）→ e2e（合成 transcript）の順に赤緑リファクタ

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（単体2件＋統合1件。analyze()直結 — 既存opencodeテストと同規約）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync済み。run_path規約とnesting対象外を明記）
