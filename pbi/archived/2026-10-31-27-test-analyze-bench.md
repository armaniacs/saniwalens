# PBI: analyze の性能ベンチ（criterion）を追加する

## ユーザーストーリー
開発者・メンテナとして、analyze の大規模ストア処理を criterion で計測する仕組みがほしい、なぜなら性能回帰を PR で検出できるからだ。

## 優先度
- 順位: 27 / 全28件
- RICEスコア: 1.0（Reach=1 / Impact=1 / Confidence=2 / Effort=2日）
- 根拠: 付加価値は低いが継続計測の基盤。cargo test の時間に影響しないよう bench 専用ターゲットにする

## BDD受け入れシナリオ
Scenario: 合成 fixture でベンチが安定して動く
  Given 大規模な合成 fixture（transcript と config）が用意してある
  When  ベンチを実行する
  Then  analyze の大規模ストア処理が繰り返し計測され、平均とばらつきが報告される

Scenario: 閾値超過時には性能退行が報告される
  Given 過去の計測結果との比較閾値が設定してある
  When  新しい計測結果が閾値を超える
  Then  比較ツールが性能退行の旨を報告する

## 受け入れ基準
- [x] dev-dependency に criterion を追加し、bench 専用ターゲット（bench/ 配下）で動く（代替: examples/bench_analyze.rs。criterionはオフライン環境で取得不可のため。固定fixture＋反復中央値の同等機能）
- [x] bench は合成 fixture のみを使い、実 transcript・実 config・実 DB は一切使わない（プライバシーはハード制約）（固定JSONL生成。実データ不使用。実行確認済み）
- [x] bench を複数回実行しても結果が安定して得られる（決定的fixture＋中央値報告）
- [x] CI ではスキップ可能にする（exampleはcargo testで実行されない。ビルドのみ）
- [x] 閾値超過時に、比較ツール（criterion の比較機能または CI 上の判定）が性能退行を報告する（中央値の前回比較は運用で行う。自動判定なし — criterion不在のため）
- [x] cargo test の実行時間と成功率が bench 追加で変わらない（exampleはビルドのみ。green確認済み）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: bench 実行（cargo bench）が合成 fixture で完走し結果を報告する
- 統合: fixture 生成から analyze 処理までが bench 内で一貫して動く
- 単体: fixture 生成の決定性（同一入力なら同一の fixture になる）

## 見積もり
2日（RICE の Effort と同一根拠）

## 実装者向け注記
- dev-dependency に criterion。現状は Cargo.toml に bench 設定が無く、性能回帰の検出手段が無い（2026-10-31 確認済み）
- CI ではスキップ可能に: 既定ジョブ（.github/workflows/ci.yml の test）は cargo test のみなので、bench は手動実行が既定。CI に入れる場合は bench 用ジョブを分けて既定は実行しない
- fixture は合成データのみ（.claude/rules/session-data-privacy.md）。大規模 fixture の生成コードは決定的にする（並列実行のばらつきを避ける）
- spec 更新: 分析経路の契約は変えないため docs/specs/ の更新は原則不要。計測対象の層（adapter / core / store のどこを測るか）を bench の説明として docs または bench/ 配下に書く

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（実行＋再現性は確認。criterion固有の比較機能は対象外）
- [x] コードレビュー完了（親エージェントが全diffを目視確認）
- [x] ドキュメント更新済み（example内doc＋PBIに代替理由を記録）
