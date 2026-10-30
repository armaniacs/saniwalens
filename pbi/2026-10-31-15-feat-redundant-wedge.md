# PBI: Redundant ウェッジを実装する

## ユーザーストーリー
skills や rules の整理をしたい saniwalens ユーザーとして、description 由来の類似度で「重複している surface」を指摘し merge を示唆する機能がほしい、なぜなら役割が重なった skills / rules を統合して設定を整理できるからだ。

## 優先度
- 順位: 15 / 全28件
- RICEスコア: 4.0（Reach=3 / Impact=2 / Confidence=2 / Effort=3日）
- 根拠: 類似度シグナルが v1 に無く（docs/specs/surfaces.md）、取得方法と格納先の設計判断が残るため確信度は中。依存: description は config アダプタが既に読んでいるため入力解析の新設は不要だが、カタログへの格納拡張が前提（実装者向け注記参照）。

## BDD受け入れシナリオ
Scenario: 類似した description の surface ペアが提示される
  Given 設定に、description の内容が互いに似ている 2 つの surface が存在する
  When  ユーザーが重複候補の一覧を表示する
  Then  その 2 つが重複候補のペアとして提示され、互いに役割が重なっている旨と merge の示唆が載る
  And   ペアには判断材料（どちらが使われているかなど）がともに示される

Scenario: 類似度がしきい値未満のペアは提示されない
  Given 設定に、description が互いに似ていない surface しか存在しない
  When  ユーザーが重複候補の一覧を表示する
  Then  重複候補のペアは提示されない
  And   重複候補が 1 件も無い場合は、その旨が分かる空の表示になる（裸の見出しだけを出さない）

## 受け入れ基準
- [ ] description 由来の類似度で、重複している surface のペアが提示される。ペアには互いの名前・所有スコープと merge の示唆を載せる
- [ ] 類似度がしきい値未満のペアは提示されない（しきい値は既定値を持つ）
- [ ] 提示までとする。merge を強制しない（ファイルの書き換え・削除・自動統合は行わない）
- [ ] 類似度シグナルの格納先を決めて実装する（attrs_json 相当の列に description を載せるか、類似度を直接格納するか。判断は spec 更新とセット。v1 の外に出す判断にする場合は根拠を spec に残す）
- [ ] waste ビュー（flagged opportunities）に redundant の候補が現れ、既存の unused / costly+rare 等の出力は変わらない
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: 似た description を持つ surface 同士が合成ストアで重複候補として現れ、似ていない場合は現れないことをユーザーシナリオとして確認する
- 統合: カタログの description から類似度が計算され、waste の候補一覧に乗ること（所有スコープの扱いを含む）を確認する
- 単体: 類似度計算（同一・似通う・無関係・description 空）、しきい値の境界（ちょうどしきい値・未満）、ペアの重複排除（同じペアを 2 回上げない）をテストする

## 見積もり
3日（RICE の Effort と同一根拠）

## 実装者向け注記
- docs/specs/surfaces.md の Redundant 節（97 行目の表、137 行目から 139 行目の段落）が「v1 スキーマに類似度シグナル無し。attrs_json に置くか、AI 層と同じく v1 の外に限る」と明記。spec 更新必須（動作変更と同じ変更内で、類似度の定義・しきい値・格納先を surfaces.md に書く）
- 現状コードの確認（2026-10-31 実施済み）: surfaces テーブル（src/store.rs 76 行目付近）に description も attrs_json も現状無い。アダプタは frontmatter description を既に読み、トークン数だけを startup_tokens に格納している（src/adapter/config.rs 194 行目付近の frontmatter_description）。description 本文（または類似度シグナル）をカタログに格納する拡張が前提
- attrs_json は「report が要るまで先送り」の 1 列として想定済み（src/store.rs 5 行目付近）。格納先の候補: attrs_json 相当の列に description を載せる / 類似度を直接格納する / 範囲外とする判断を含める
- 類似度の計算は純粋関数にする（core に置き、I/O フリー）。外部依存（埋め込み API 等）は持ち込まない
- 提示先は waste ビュー（src/cli.rs、docs/specs/cli.md の Views 表）。merge の示唆を載せるが実行はしない（計測して見せるの枠内）
- TDD: 単体（類似度・しきい値境界）→ 統合（waste への配線）→ e2e（合成 fixture）の順に赤緑リファクタ
- プライバシー: 実 config の description はコミットしない。合成 fixture のみ

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み
