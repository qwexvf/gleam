# Contracts v0: 実装と評価

仕様は `SPEC-v0.md`。ブランチ `proof` (未コミット)、upstream `69822bed9` からの差分は既存ファイルとスナップショットで下記、新規 `compiler-core/src/verify.rs` が約 650 行。

## 実装したもの

- `parse.rs`: `@requires(<式>)` / `@ensures(<式>)` アトリビュートを読む。関数以外に付けると構文エラー。
- `analyse.rs`: 型検査の前に契約を `assert` (`AssertKind::Requires` / `Ensures`) に書き換える。型検査と実行時チェックは既存の仕組みのまま。
- `verify.rs`: 契約を持つモジュールの各関数から検証条件を作り、SMT-LIB にして z3 プロセスで解く。呼び出しは相手の契約だけで検証する (modular)。モデル化しない式は制約なしの値として扱う。
- `package_compiler.rs`: 型検査に成功したモジュールに対して検証を走らせ、証明できなかった義務を警告にする。Erlang target のみ。
- `warning.rs`: 反例付きの警告と、z3 が起動できない場合の警告。
- `format/`: 契約をアトリビュートとして出力する。関数本体とシグネチャの書式は upstream のまま。
- `call_graph.rs`: 契約の式の中の呼び出しも型検査の順序に入れる。

## 評価

例は `examples/ctest.gleam`、独立オラクルは `examples/oracle.erl`。solver は pip の z3-solver 5.3.3.0 に同梱されたバイナリ (`Z3 version 5.3.3`) を `GLEAM_Z3` で指定した。

### 1. 互換性

| 確認 | 結果 |
|---|---|
| `let requires = 3` を含む既存スタイルのコード | ビルドと実行が通った |
| `gleam_stdlib` 1.0.5 / `gleeunit` 1.11.0 を fork でソースからビルド | 通った |
| `gleam format` で契約が残るか | 残った。`examples/ctest.gleam` に対して出力は入力と同一 |
| 型に `@requires` を付ける | 構文エラー「I was expecting a function definition after this」 |
| upstream のテスト (`gleam-core`, `gleam-format`) | 3626 + 549 + 2 件すべて成功。AST の Debug 出力が変わるパーサのスナップショット 11 件を更新 (追加行は `kind: Plain` 7、`requires: []` 4、`ensures: []` 4 のみ)。著作権表記テストは `proof-design/` を対象外にした |

### 2. 検証結果と BEAM 上の総当たりの一致

オラクルは生成された Erlang を BEAM で実行し、`requires` を満たす入力 (-40..40 の範囲) で実行時の assert 失敗を数える。SMT のエンコードとは独立に動く。

| 関数 | 検証器 | BEAM 総当たり |
|---|---|---|
| `withdraw` | proved | 820 入力で失敗 0 |
| `withdraw_with_fee` (実装バグ) | 反例 `balance=1, amount=1` | 40 失敗、最初は `[1,1]` |
| `safe_withdraw` (呼び出し側でガード) | proved (呼び出し先 requires 2件 + ensures) | 3321 入力で失敗 0 |
| `careless` (ガード忘れ) | 反例 `balance=-1` と `balance=9` | 81 入力中 50 失敗 |
| `half` (`n/2`, `n<0`, `result*2 >= n`) | proved | 失敗 0 |
| `half_exact` (陰性対照) | 反例 `n=-1` | 20 失敗 |
| `sum_to` (再帰) | proved (再帰呼び出しの requires + ensures) | 失敗 0 |
| `sum_to_strict` (陰性対照) | 反例 `n=0` | 41 入力すべて失敗 |
| `rem_sign` (陰性対照) | 反例 `a=-1, b=2` | 2884 失敗 |

範囲内では 9 関数すべてで一致した。言えるのは「範囲 -40..40 で矛盾がない」までで、範囲外の正しさはこの比較では示せない。オラクルが使う実行時 assert は検証器と同じ脱糖 (`desugar_contracts`) から来るので、脱糖そのものの誤りはこの比較では見つからない。

#最初の実装は契約をシグネチャの後に書く構文 (`-> Int requires ... {`) だった。関数の書式を変えないためにアトリビュートへ移し、同じ例とオラクルを再実行して結果が変わらないことを確認した。

## 3. 途中で見つかったバグ

最初の版は `withdraw_with_fee` を proved と報告した。証明した `ensures` を直後に事実として追加し、その事実がその義務自身の問い合わせにも入っていたため。各問い合わせには、義務より前に記録した事実だけを渡すよう直した (`Query::facts`)。陰性対照を置かなければ見逃していた。

型に付けた `@requires` が黙って無視されていた。カスタム型は `@external` を受け付けるので、既存の「関数専用アトリビュート」の判定を通り抜けていた。契約アトリビュートは関数以外ではエラーにした。

`half` は Gleam の除算 (0 方向への切り捨て) では真、SMT-LIB 組み込みの `div` (ユークリッド除算) では偽になる。プレリュードの `gdiv` / `grem` が効いていることの確認にもなっている。

## まだないもの

- 検証器自体の Rust 単体テスト。今の確認は `examples/` の手動実行とオラクルだけ。

## 既知の制約 (v0)

- 別モジュールの関数の契約は使えない (package interface に載せていない)。
- `case` のガード、代替パターン、リスト、タプル、レコード、`let assert` はモデル化していない。その中に隠れた呼び出しの `requires` は検証されず、実行時チェックだけになる。
- 契約付きの関数は `let result = {...}` で包まれるので、末尾呼び出しでなくなる。
- 証明できた契約も実行時チェックを消していない。
- 同じ呼び出しで複数の `requires` が破れると、警告が複数出る。
- z3 の起動は義務 1 件ごとに 1 プロセス。
- JS target では検証しない。
