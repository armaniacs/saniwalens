<!-- The shared skeleton. docs/pr/ keeps the hand-written bilingual PR prose;
     this template is the common bones, not a replacement. -->

## 動機 / Motivation

<!-- なぜこの変更が要るか。仕様・PBI・issue があれば番号で指す。 -->

## 検証 / Verification

<!-- 動かした検証コマンドと結果。最低でも以下。 -->

```sh
just check   # rustfmt --check + clippy(CI の関門)
just test    # cargo test
```

<!-- 上以外に動かした確認があれば追記(E2E、capture の diff など)。 -->

## 確認項目 / Checklist

- [ ] 振る舞いを変えた場合、所有する spec(`docs/specs/`)を同一変更で更新した
- [ ] 新しい振る舞いにテストを足した(合成 fixture のみ。実 transcript・実 config は入れない)
- [ ] `docs-charset` の文字種に収まっている(CP932 で書ける文字のみ)
