# PBI: MCP tool schema のトークン計測（unknown schema の解消）

## ユーザーストーリー
MCP server を使う saniwalens ユーザーとして、MCP server 設定から tool schema の静的コストを計測する機能がほしい。なぜなら「unknown schema」ランク（計量できず次点に置かれる）を実測に解消し、MCP のコストを他の設定 surface と正しく比較できるようになるからだ。

## 優先度
- 順位: 19 / 全28件
- RICEスコア: 2.7（Reach=2 / Impact=2 / Confidence=2 / Effort=3日）
- 根拠: 計測精度の改善だが、schema を取得できるかどうかに不確実さが残るため。依存関係はなし（既存の surface カタログとコスト ranking の延長）。

## BDD受け入れシナリオ

Scenario: tool schema の静的コストが計測され、measured tokens のランクに並ぶ
  Given tool schema を静的に読める MCP server が設定されている
  When  ユーザーが surface の一覧と最適化候補を表示する
  Then  その server の tool schema の静的コストがトークン数で表示される
  And   その server は「measured tokens」のランクに並び、「unknown schema」扱いでなくなる

Scenario: schema を読めない server は従来どおり unknown schema として flag される
  Given リモートのみで動くなど、静的に schema を読めない MCP server が設定されている
  When  ユーザーが surface の一覧と最適化候補を表示する
  Then  その server は従来どおり「unknown schema」として flag される
  And   静的コストが 0 として表示されたり、偽の見積もりに置き換えられたりはしない

## 受け入れ基準
- [ ] tool_schema の静的コストが計測され、measured tokens のランクに並ぶ
- [ ] スキーマを読めない server（リモートのみ・認証が必要など）は従来どおり unknown schema として flag され、静的コスト 0 や偽の見積もりを表示しない
- [ ] schema 取得の方式（どこから読むか、読めない場合の扱い）が docs/specs に記録される
- [ ] MCP server の tool name と usage の対応（既存の結合キー）が崩れない
- [ ] cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test が green

## テスト戦略
- E2E: 合成 fixture の MCP server 設定で analyze を実行し、読める server の tool schema コストが表示され、読めない server が unknown schema として flag される
- 統合: 取得した schema が静的コスト付きで surface カタログに載り、measured tokens ランクの並び順に反映される。読めない server は計量不能のまま記録される
- 単体: schema テキストのトークン計測の境界（空の schema、巨大な schema、取得失敗時の扱い）、measured tokens / unknown schema / declutter の 3 ランクへの振り分け

## 見積もり
3日（RICE の Effort と同一根拠：schema 取得方式の discovery 込み）

## 実装者向け注記
- spec 更新必須: docs/specs/surfaces.md の「unknown schema」節（114 行付近）と docs/specs/config-format.md の「The MCP tool-schema hard case」節（139 行付近）。config-format.md は現状「ローカルデータからは計量不能（the honest limit）」という過去の調査結果を記載しているため、schema 取得方式の決定と合わせて書き換えること。決定に至った根拠も記録する。
- 契約の前提: config-format.md は「schema source があれば他の surface と同様に計量し、なければ unknown コストで記録して report が flag する（偽のゼロを出さない）」と best-effort として契約済み。実装するのは schema source の読み取りそのもの。
- 取得方式の候補は discovery で比較すること。偽の見積もり（tool 数 × 平均サイズ等）は spec が禁じる fabricate なので作らない。
- adapter の役割: MCP 設定の読み取りは adapter。現状は src/adapter/config.rs の read_mcp_server_surfaces（465 行付近）が mcp.json から mcp_server surface を組み立て、server 名を tool name 用に sanitize している（498 行付近）。tool_schema surface の組み立てと schema 読み取りも adapter に置き、I/O は core に漏らさない。
- core: 計量結果は Surface の static_tokens / startup_tokens（src/core/surface.rs 88 行付近）に載る。3 ランクの ranking は docs/specs/surfaces.md の「Ranking by what removal actually saves」節。unknown のときの None 扱い（src/adapter/config.rs 772 行付近のテスト「an_mcp_server_startup_cost_is_unknown_not_zero」）は、読めない server について維持する。
- privacy: server との接続や schema 取得で秘密情報を取得・コミットしない。fixture は合成（.claude/rules/session-data-privacy.md）。
- 検証コマンド: cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test。

## Definition of Done
- [ ] 全BDDシナリオが自動テストとして実装されパスする
- [ ] コードレビュー完了
- [ ] ドキュメント更新済み

## Discovery 結果（2026-11-08）: 安全な取得元が無いため計測不可を確認

PBI 指示の discovery を実データで実施した。結論: 静的・安全に読める
schema ソースは存在しない。

- `.mcp.json` は launch 設定（`command`/`args`/`env`）のみ。実例 3 件は
  いずれも `uvx ... start-mcp-server` 形式で、schema を含まない。
- オンディスクの schema キャッシュは無い（`~/.claude/cache/` に MCP
  関連のキャッシュ無し。`mcp-needs-auth-cache.json` は auth 状態のみ）。
- transcript に載るのは *使われた* tool の input 値であり schema そのもの
  では無い。未使用 server（削減検討の主対象）は transcript に痕跡を残さない。
- ライブ問い合わせ（ListTools）は server プロセスの起動＝任意コード実行に
  なる。静的解析ツールが分析のたびに `uvx ... start-mcp-server` を実行する
  のはセキュリティ上不可のため、取得手段として採用しない。

よって PBI の受け入れ基準 1（measured 化）は満たせない。既存の誠実な代替は
維持する: `unknown schema` への分類（`an_mcp_server_startup_cost_is_unknown_not_found`
相当のテストで固定済み）、`inventory` での計量不能表示、`overhead` 残差に
よる上限推定。誤ったゼロや推定値の捏造は行わない（spec の honest limit 宣言どおり）。

着手条件: 安全な静的 schema ソース（キャッシュ規約の確立、または
transcript への schema 記録）の出現。以後の schema 取得の議論はこの記録を起点にすること。
