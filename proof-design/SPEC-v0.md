# Contracts v0

最初の評価用の言語仕様。既存の Gleam コードと Hex パッケージは変更なしでビルドできること (上位互換) を条件にする。

## 構文

関数の書式は変えない。契約は `@external` や `@deprecated` と同じアトリビュートとして関数の上に書く。

```gleam
@requires(balance >= 0)
@requires(amount > 0 && amount <= balance)
@ensures(result >= 0)
pub fn withdraw(balance: Int, amount: Int) -> Int {
  balance - amount
}
```

- `@requires(<式>)` と `@ensures(<式>)` は何個でも書ける。式は `Bool` 型の普通の Gleam 式。
- `@ensures` の中では `result` が返り値を指す。引数に `result` があれば、それを隠す。
- 関数以外 (型、定数) に付けると構文エラー。
- upstream の gleam は未知のアトリビュートを拒否するので、契約を書いたファイルは fork でしかビルドできない。契約を書いていない既存コードと Hex パッケージはそのままビルドできる。

## 意味

型検査の前に、本体を次の形へ書き換える。

```gleam
{
  assert <requires 1>      // kind: Requires
  ...
  let result = { <元の本体> }
  assert <ensures 1>       // kind: Ensures
  ...
  result
}
```

- 型検査は既存の `assert` の仕組みをそのまま使う。
- 証明できなかった条件は実行時チェックとして残る (gradual)。証明できた条件は実行時チェックを消す (v0 では消さず、警告の有無だけ見る)。
- 呼び出し側は、相手の本体ではなく契約だけを使って検証する (modular)。呼び出し前に相手の `requires` を証明し、呼び出し後は `ensures` を仮定する。

## 検証

- `requires` / `ensures` を持つ関数ごとに検証条件を作り、SMT-LIB のテキストとして z3 プロセスに渡す。z3 は `GLEAM_Z3` 環境変数、なければ `PATH` 上の `z3`。
- v0 で扱う式: `Int` と `Bool` のリテラル、変数、`+ - *`、比較、`&& || !`、`let`、`Bool` と `Int` に対する `case`、`if` 相当の分岐、契約を持つ関数の呼び出し。
- 整数除算と剰余は Gleam の意味論 (0方向への切り捨て、`x / 0 == 0`) でエンコードする。
- 対象は Erlang target のみ。JS target の `Int` は float64 なので、v0 では検証しない。
- 扱えない式は制約のない値として扱う。証明が通らなくなるだけで、誤って通ることはない。その中に隠れた呼び出しの `requires` は検証されず、実行時チェックだけになる (報告もしない)。
- 結果は警告として出す: 反例 (引数の具体的な値) か、未知 (timeout など)。

## v0 の既知の制約

- `ensures` のために本体を `let result = ...` で包むので、再帰関数の末尾呼び出し最適化が失われる。
- `assert` は関数を impure にするので、契約を書いた関数も impure になる。
- package interface (`metadata`) に契約は載らない。別パッケージの関数の契約は使えない。
