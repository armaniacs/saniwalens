# PBI: rule 読み取りの symlink 再帰を修正する

## ユーザーストーリー
clone したリポジトリを分析する saniwalens ユーザーとして、rule 配置に
循環 symlink が混ざっていても、分析がクラッシュせずに終わり、問題箇所を
知らせる警告が出る状態がほしい。なぜなら外部由来の配置で落ちると、
分析自体ができなくなり、DoS として成立してしまうからだ。

## 優先度
- 順位: 03 / 全17件
- RICEスコア: 9.6（Reach=3 / Impact=2 / Confidence=80% / Effort=0.5日）
- 根拠: clone したリポジトリの内容から到達できる crash DoS。
  再帰に上限が無く、スタック枯渇で落ちる実害がある。

## BDD受け入れシナリオ
Scenario: 通常の入れ子になった rule 配置はこれまで通り全部読める
  Given カテゴリのサブディレクトリに分かれた通常の rule 配置がある
  When  ユーザーが分析を実行する
  Then  すべての rule がカタログに載る
  And   警告は出ない

Scenario: 循環 symlink があってもクラッシュせず分析が終わる
  Given rule ディレクトリ内に循環する symlink がある
  When  ユーザーが分析を実行する
  Then  分析は正常に終わる
  And   循環箇所は読み飛ばされ、警告が出る

Scenario: 異常に深い入れ子でもスタック枯渇で落ちない
  Given 通常では考えられない深さまで入れ子になった rule 配置がある
  When  ユーザーが分析を実行する
  Then  分析は正常に終わる
  And   上限を超えた先は読み飛ばされ、警告が出る

## 受け入れ基準
- [x] rule 再帰の読み取りは循環で無限再帰しない（訪問済み判定で打ち切る）
- [x] 深さに上限があり、異常な深さでもスタック枯渇で落ちない
- [x] 循環や上限超過の箇所は読み飛ばし、警告を出して続行する
- [x] 通常の入れ子配置の読み取り結果は変わらない（既存テストと capture が green）
- [x] @import のホップ上限とは独立した扱いであることがテストで分かる
  （既存の上限を流用しただけに見えないこと）
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: 再帰関数の境界テスト。循環 symlink、自己参照、深い入れ子、
  通常の入れ子の各形を合成ディレクトリで作り、終わることと警告・打ち切りを
  固定する。Unix 固有の symlink テストは既存の cfg(unix) 規約に従う
- 統合: read_project_surfaces 経由で、循環配置を含む合成プロジェクトの
  カタログが正常に組み立てられることを確認する
- E2E: 循環配置を含む合成リポジトリの analyze がクラッシュせず終わること。
  正常配置のリポジトリの出力差分が無いこと

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 再帰の箇所: src/adapter/config.rs:303-311 の collect_rule_surfaces。
  entry の is_dir 判定でディレクトリを辿り直すが、is_dir は symlink を
  追うため、循環 symlink で無限再帰になる。visited-set も深さ上限も無い
- 上限の対比: src/adapter/config.rs:332 の MAX_IMPORT_HOPS は @import 側の
  ホップ制限であり、この再帰には効かない。別物の上限として扱い、
  既存の import 側テスト（cycle は一度だけ計上、深さ超過は切り捨て）と
  対になるテストを rule 側にも作る
- 修正方向: 正規化したディレクトリの visited-set + 深さ上限。循環は
  読み飛ばして警告して続行する。警告の出し方は既存の read_dir_warn
  （src/adapter/config.rs:63-75）のトーンに合わせる
- 既存の対称例: src/adapter/config.rs:382-440 の read_claude_md_surfaces は
  canonicalize 済みパスの seen-set と幅優先 + ホップ上限で循環と深さを
  抑えている。同型の考え方を rule 側にも適用する方向
- 関連コード: src/adapter/config.rs（read_rule_surfaces、collect_rule_surfaces、
  read_dir_warn）、呼び出し元の read_project_surfaces
  （src/adapter/config.rs:447-460）
- エビデンス: docs/specs/config-format.md（rule 配置と再帰の記述）、
  既存の symlink / import テスト（src/adapter/config.rs の tests）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（契約不変のためspec更新なし。config-format.mdの再帰記述どおりに振る舞う）
