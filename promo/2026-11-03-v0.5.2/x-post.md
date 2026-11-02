# X ポスト案 ― v0.5.2リリース

認証ユーザーの長文ポストとして書いた。XはMarkdownを解釈しないので見出しは「■」、区切りは空行と改行で作る。画像は録画未作成のため既存GIFで代用と明記。開発基盤の変更はXに載せない。

## 案1: リリースのまとめ（メイン）

添付: trend実行画面の短い録画1枚（未作成のため公開前に撮影。なければ既存GIFで代用）。文字数: 約400字

saniwalens v0.5.2 をリリースしました。コーディングエージェントの設定の無駄を見つけるCLIです。

■ 変わったこと
・trendビュー:直近の使い方の増減を前期間との差分で出す
・期間フィルタ:8つの表示を--since/--until/--lastで絞れる
・optimizeに--timeout:長い実行を期限で切って記録を示す

対応はClaude Code、opencode、Kilo Codeの3つ。DSHは次の候補です。

あなたの代わりに直す道具ではなく、どこを直すと効くかを測って見せる道具です。

リリースノート: https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2
配布: https://github.com/armaniacs/saniwalens/releases

#saniwalens

## 案2: trendビュー

添付: trend表出力の静止画1枚（未作成のため公開前に撮影）

使い方の増減が、前期間との差分で見えるようになりました。

saniwalens trend で直近7日とその前の7日を比べます。増えたスキル、減った使い方が表で分かります。比べる相手がいないときは「比較不能」と出ます。

リリースノート: https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2

## 案3: 期間フィルタ

添付: --last付きdoctor実行画面の静止画1枚（未作成のため公開前に撮影）

8つの表示を期間で絞れるようになりました。

saniwalens doctor --last 14d のように付けます。絞った期間は表示にも出るので、いつの話か分からなくなりません。

リリースノート: https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2

## 案4: fork後の対応ハーネス

添付: 対応表の静止画1枚（未作成のため公開前に撮影。合計4枚まで）

cclensからforkしたsaniwalensは、Claude Code、opencode、Kilo Codeの3つに対応しました。

--harnessで切り替え、結果はハーネスごとに別ストアで混ぜません。Claude専用には元のcclensもあります。

リリースノート: https://github.com/armaniacs/saniwalens/releases/tag/v0.5.2

## 投稿の手順

- 案1を最初に出す。案2以降は反応を見ながら数日あけて単独で出す
- 案1のスレッドにするなら「■ 変わったこと」を機能案へぶら下げる
- ハッシュタグは#saniwalens1個のみ、本文最終行に置く。案1は約400字のため上限内に収まる
- 画像は合計最大4枚。各案1枚ずつ、録画は公開前に撮影しなければ既存GIFで代用と明記
- 公開前にリリースノートURL(タグ発行後に有効)の有効確認をする
- 予約投稿時はURLと画像添付の有無を再確認する
