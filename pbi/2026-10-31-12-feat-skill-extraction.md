# PBI: スキル抽出（繰り返しプロンプトのクラスタリング）を実装する

## ユーザーストーリー
saniwalens ユーザーとして、繰り返しのユーザープロンプトをクラスタリングし、新スキル候補を提示する機能がほしい、なぜなら手で気づかなかった「スキル化すべき定型作業」をデータから発見したいから。

## 優先度
- 順位: 12 / 全28件
- RICEスコア: 4.8（Reach=4 / Impact=3 / Confidence=2 / Effort=5日）
- 根拠: 価値は高いがクラスタリング品質の確信度が中で工数大（E=5日）。07 から 11 の CI・文書系とは独立だが、品質設計の確定が必要なので後ろに置く。

## BDD受け入れシナリオ
Scenario: しきい値以上繰り返したプロンプト群が候補として提示される
  Given ユーザープロンプトがしきい値以上の回数、同じ趣旨で繰り返し記録されている
  When  ユーザーがスキル抽出を実行する
  Then  繰り返しのプロンプト群が 1 つの新スキル候補として提示され、候補ごとに繰り返し回数と例示が添えられる

Scenario: 参照先の transcript が回転済み（原文が無い）場合も部分結果を出す
  Given 一部のプロンプトについて、参照先の transcript が回転済みで原文が残っていない
  When  ユーザーがスキル抽出を実行する
  Then  残りのデータだけで部分結果を出し、原文が取り出せなかった件数を注意書きとして添える。エラーで全結果を捨てない

## 受け入れ基準
- [x] しきい値以上繰り返したユーザープロンプト群が新スキル候補として提示される（promptsビューの候補節。E2Eで反復を確認）
- [x] 参照先の transcript が回転済みで原文が取り出せない場合も、残りデータで部分結果を出し、注意書きを添える（pruned计数＋注意書き。E2Eで確認）
- [x] プロンプト本文はストアに保持しない（参照のみ。提示時にtranscriptから回収）
- [x] 結果は提示まで。候補に対する具体的な指示（advise）はしない（表示のみ。advisor起動なし）
- [x] しきい値の境界（ちょうど・未満）と、空データ・ゼロ件で破綻しない（単体テストで固定）
- [x] 既存コマンドの出力はpromptビューの候補節追加のみ。既存キーは不変（JSONにcandidates/pruned_prompts追加）
- [x] 合成 fixture で検証する。実 transcript・実 config は使わない（全テスト合成のみ）
- [x] just check と cargo test が green
- [x] docs/specs/ の該当節を同じ変更内で更新する（architecture抽出節＋cli.md prompts行）

## テスト戦略
- E2E: 繰り返しプロンプトを含む合成ストアで抽出を実行し、候補が提示されること。回転済みデータが混在するストアで部分結果と注意書きが出ること
- 統合: 参照（ファイル名と行位置）からの原文回収が合成 transcript で正しく行われること、回転済み参照の判定が正しいこと
- 単体: クラスタリングの境界値（しきい値ちょうど・未満、同じ趣旨の判定）、回転済み参照の扱い、空データ・ゼロ件の例外

## 見積もり
5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 前提の記録は済んでいる: ストアは prompt イベントに参照先（source_path, source_line）を既に保持する（docs/specs/storage.md の「Prompt text is referenced, never copied」節、docs/specs/architecture.md の「Skill extraction」節）。本 PBI はその消費側（クラスタリングと提示）を実装する
- privacy 姿勢: 原文は transcript から必要時に回収し、ストアには本文を保持しない。回収結果も永続化せず表示に留める（.claude/rules/session-data-privacy.md）。fixture は合成
- 哲学: 結果は提示まで（advise しない）。docs/specs/architecture.md の「Optimization proposals」（optimize の担う領域）との境界を混同しない。スキル抽出は新表面の候補発見
- E が大きい理由: クラスタリング品質の設計（しきい値、同じ趣旨の判定、言語差の扱い）が要る。まず合成 fixture で判定基準を固定してから実装し、Confidence=2 の理由を設計で潰す
- spec 更新: 必要（新機能の契約を docs/specs/ の該当節に同じ変更内で追記。spec-sync 規約）
- 関連コード: src/store.rs（prompt イベントの参照保持）、src/core/（クラスタリング集計は純粋・I/O フリーに）、src/adapter/（transcript からの原文回収は adapter の知識）、src/cli/mod.rs（新ビューの配線）
- 落とし穴: 原文が取り出せない参照はエラーではなく注意書き（transcripts rotate、storage.md の記述どおり参照先が無くなってもイベントの件数は残る）。先の trend の「比較不能」明示と同じ報告の誠実さ（近似は隠さずラベルする）に揃える

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする（E2E 2件＋core単体）
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（spec-sync済み）
