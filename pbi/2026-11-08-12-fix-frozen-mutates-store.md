# PBI: --frozen がストアを変更できる問題を直す

## ユーザーストーリー
`--frozen` で読み取り専用を期待する saniwalens ユーザーとして、frozen 実行が
ストアに書き込まず、読み取り専用ファイルシステム上でも失敗しないでほしい。
なぜなら「変更しない」と約束した読み取りが書き込みで失敗するのは契約違反
だからだ。

## 優先度
- 順位: 12 / 全17件
- RICEスコア: 3.2（Reach=2 / Impact=1 / Confidence=80% / Effort=0.5日）
- 根拠: --frozen が読み取り専用ファイルシステムで失敗し、変更しない約束を
  破る。修正は経路変更のみで小さい。

## BDD受け入れシナリオ
Scenario: frozen 読み取りはストアを変更しない（happy path）
  Given 分析済みのストアがあり
  When  ユーザーが `--frozen` で読み取りビューを実行する
  Then  analyzer は実行されない
  And   ストアファイルの内容・mtime・パーミッションが変化しない

Scenario: 読み取り専用ファイルシステム上でも frozen は成功する（error/boundary）
  Given ストアのあるディレクトリが読み取り専用である
  When  ユーザーが `--frozen` で読み取りビューを実行する
  Then  ビューが正常に表示される
  And   chmod や migration のための書き込みは行われない

Scenario: 通常の読み取りは従来通り refresh する
  Given 分析済みのストアがあり
  When  ユーザーが `--frozen` 無しで読み取りビューを実行する
  Then  analyzer による refresh が実行される

## 受け入れ基準
- [x] frozen 経路で `Store::open(..., OpenMode::ReadWrite)` が呼ばれないこと
- [x] frozen 経路で ensure_owner_only の chmod、migrate、legacy-store 移動が
  発生しないこと
- [x] frozen でも freshness 行の表示挙動が維持されること
- [x] 非 frozen の refresh 挙動が変化しないこと
- [x] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、
  cargo test が green

## テスト戦略
- 単体: frozen 経路が read-only open を使うことを固定する。既存の
  `open_read_frozen_never_runs_the_analyzer`（analyzer 不実行のアサートのみ）
  に加え、読み取り専用に chmod した合成ストアでも open_read(frozen) が
  成功することを追加する
- 統合: frozen 実行の前後でストアファイルの mtime と内容ハッシュが不変で
  あること。pure-default 解決での migrate_legacy_store が frozen では
  発火しないこと
- E2E: 読み取り専用マウント相当（書き込み不可 dir）で `--frozen` ビューが
  成功し、非 frozen が従来通り refresh すること

## 見積もり
0.5日（RICE の Effort と同一根拠）

## 実装者向け注記
- 原因は src/session.rs:158-160 の frozen 分岐が `Self::open` を呼ぶこと:
  `Self::open` は resolve_path（migrate_legacy_store を含む :220-239）から
  `Store::open(..., OpenMode::ReadWrite)`（:563-573 のディレクトリ作成と
  ensure_owner_only chmod、from_connection 経由 :630-632 の migrate）に至る
  書き込み経路である。無変更契約を持つのは `OpenMode::ReadOnly`（:574-584、
  「No create, no chmod, no migration」）を使った open_readonly だけであり、
  既存の frozen テスト（:677-698）は analyzer 不実行のみをアサートして
  書き込み有無を見ていない
- 修正方向: frozen 分岐を read-only open 経路（open_readonly と同等）に
  通す。freshness 行（`eprint_freshness`）の表示挙動は保つ。`--db` 明示と
  pure-default の両解決で read-only になることを確認する
- guard_harness の読み取りは read-only 接続で問題ないことを確認する。
  refresh 経路（:195-218）は非 frozen 専用に残す
- エビデンス: docs/specs/cli.md（--frozen の無変更契約）、
  docs/specs/storage.md（migration と owner-only 規約）
- 関連コード: src/session.rs（open_read / open / open_readonly /
  resolve_path / migrate_legacy_store）、src/store.rs（Store::open の
  OpenMode 分岐と migrate）

## Definition of Done
- [x] 全BDDシナリオが自動テストとして実装されパスする
- [x] コードレビュー完了（バッチ統合レビュー: 親エージェントが全diff・spec同期・干渉を目視確認）
- [x] ドキュメント更新済み（契約不変: specは既にread-only記述。コードをspecに合わせた）
