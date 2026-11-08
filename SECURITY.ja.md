# セキュリティポリシー

## サポートするバージョン

セキュリティ修正は最新のリリース系列にのみ入る。古いリリースを使っている
場合は先に上げて、問題が残るか確かめてほしい。

## 脆弱性の報告

脆弱性の疑いは公開 issue に書かず、
[GitHub Security Advisories](https://github.com/armaniacs/saniwalens/security/advisories/new)
から非公開で報告してほしい（「Report a vulnerability」)。修正が出るまで
報告はあなたとメンテナにしか見えない。

書いてほしいこと:

- 何をしたか、どうなるはずだったか。
- saniwalens の版 (`saniwalens --version`) と harness
  (`--harness claude | opencode | kilocode`)。
- 再現に実 transcript・実 config が要るかどうか。要る場合もそのまま
  貼らず、個人情報と秘密を抜いた最小の合成例に置き換えること
  (`CONTRIBUTING.ja.md`「Issues と PR」参照)。

## その後の流れ

メンテナが受けて合成データで再現し、修正リリースを出す。CHANGELOG に
報告者の名を載せる（匿名希望は除く)。報奨金制度は無い。公開は修正待ち。

## 範囲の注意

saniwalens は agent の transcript と config を手元の SQLite に読むだけ
で、外部送信はしない（crate に通信経路は無い)。機械の外にデータが出る
報告は重大扱い。自分の disk 上の自分の store の中身はファイル権限の
話で、このツールの脆弱性ではない。
