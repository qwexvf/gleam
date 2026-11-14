# Gleam proof fork: 使い方の予測、パターン、レビュー、評価

対象: `gleam-lang/gleam` `69822bed9` (2026-11-05) の fork。動作確認は gleam 1.18.1 と z3-solver 5.3.3.0 で行った。

## 1. 将来の使い方 (シナリオ)

| # | シナリオ | 誰が何を書くか | パターンへの要求 |
|---|---|---|---|
| S1 | AI ペア開発 | 人間が仕様、AI が実装と補題、Z3 が判定 | 仕様が短く、シグネチャの近くにあり、人間が読んで要件と照合できる |
| S2 | Hex ライブラリ | 作者が契約を公開、利用者はそれを前提にできる | 仕様が package interface に載る。未対応の利用者も壊れない |
| S3 | 既存コードへの段階導入 | 注釈なしは今まで通り、核心部だけ仕様化 | gradual。注釈ゼロで既存 Gleam と同じ挙動 |
| S4 | ドメイン不変条件 | `Balance >= 0`、`NonEmpty`、`Percent` | 型に一度書けば全利用箇所で効く |
| S5 | 部分関数の除去 | `let assert`、`assert`、`panic` を証明で消す | 構文追加なしで価値が出る |
| S6 | OTP actor | メッセージの種類と応答型、終了後の送信禁止 | プロトコルを型として書ける |
| S7 | Erlang/Elixir FFI | 外部の値は未検証 | 信頼境界が明示され、監査できる |
| S8 | CI と AI ループ | `gleam verify` が反例を返し、AI が修正 | 反例が具体的で機械可読 (JSON) |

## 2. パターン

実ファイルは `patterns/`。全パターンで同じ `withdraw` と `at` を書いた。

- **P1 doc comment** (`p1_doc_comment.gleam`): `/// @requires ...`。upstream gleam 1.18.1 でそのままビルドできた。
- **P2 attribute** (`p2_attribute.gleam`): `@requires(...)`。upstream は `Syntax error` で拒否した (`parse.rs:4872` の `UnknownAttribute`)。
- **P3 inline refinement** (`p3_inline_refinement.gleam`): `amount: {a: Int | a > 0 && a <= balance}`。
- **P4 named refinement + clauses** (`p4_named_refinement.gleam`): `type Natural = Int where it >= 0`、シグネチャの後に `requires` / `ensures`。
- **P5 opaque invariant** (`p5_opaque_invariant.gleam`): `pub opaque type Balance {...} invariant { value >= 0 }`。
- **P6 assert reuse** (`p6_assert_reuse.gleam`): 構文追加なし。既存の `assert` / `let assert` を証明義務にする。upstream でビルドできた。
- 横断: **C1 actor protocol** (`c1_actor_protocol.gleam`)、**C2 FFI trust** (`c2_ffi_trust.gleam`、`trusted` は境界で実行時検査、`assume` は検査なしで監査リストに載る)。

## 3. 評価: Z3 で検証条件を手書きして解いた

`eval/vc.py`、結果は `eval/results.txt`。検証条件はパターンから手で書いたもので、コンパイラが生成したものではない。示せるのは「エンコーダが何を出せばよいか、それが解けるか」まで。

| ID | 内容 | 結果 | 時間 |
|---|---|---|---|
| E1 | `withdraw` 線形契約 | PROVED / 前提欠落で反例 `b=0, a=1` | 約1ms |
| E2 | opaque 不変条件の保存 (`deposit`, `new`) | PROVED | 1ms 未満 |
| E3a | 再帰 `sum` を本体ごと渡す (帰納法が必要) | UNKNOWN (5s timeout) | 5012ms |
| E3b | 再帰呼び出しを自身の `ensures` で置換 (modular) | PROVED | 約1ms |
| E4 | `at` の境界、呼び出し側ガード | PROVED / `i <= length` で反例 `xs=[], i=0` | 1ms 未満 |
| E5 | 整数除算 | Z3 組み込みで `-7/2 = -4`、Erlang は `-3` (`erl` で確認)。同じ性質が Z3 組み込みでは PROVED、Gleam 意味論では反例 | 1ms 未満 |
| E6 | 非線形 (`w*h >= w`, `n*(n+1)` 偶数) | PROVED | 6ms 以下 |
| E7 | `x + 1 != x` | Erlang Int では PROVED、JS (float64) では反例 (`node` でも `2**53+1 === 2**53` を確認) | 18ms |

### 評価から出た設計制約

1. **検証は modular 必須** (E3a と E3b)。呼び出し側は相手の本体ではなく契約だけを使う。再帰関数には `ensures` が要る。よって呼び出し元に契約を公開できない P6 単独では足りない。
2. **エンコーダ自体が一番の誤り源** (E5)。SMT の既定の意味論は Gleam と違う (除算の丸め、`x / 0 == 0`)。エンコーダはコンパイラの出力と独立に確かめる必要がある。案: 生成した Erlang をランダム入力で実行し、Z3 のモデル評価と突き合わせる差分テスト。
3. **target ごとに意味が違う** (E7)。JS target の Int は IEEE double。最初は Erlang target 限定にし、JS は `Float` 相当のエンコードを別途用意するか未対応と明示する。
4. **UNKNOWN と反例は別物として見せる** (E3a)。timeout を「バグ」と表示しない。
5. **反例は短く具体的** (E1, E4)。AI ループの入力にそのまま使える。
6. 線形の小さな条件は 1ms 前後。LSP で保存ごとに回せる見込みはあるが、測ったのは玩具サイズの条件だけで、実モジュール規模は未計測。

## 4. レビュー: 基準ごとの採点

採点は私の判断で、評価結果 (3節) とビルド確認を根拠にした。1-5点、重み付き、満点60。

| 基準 (重み) | P1 doc | P2 attr | P3 inline | P4 named | P5 opaque | P6 assert |
|---|---|---|---|---|---|---|
| R1 仕様の読みやすさ、要件との照合 (3) | 3 | 3 | 2 | 5 | 5 | 3 |
| R2 AI が書けて反例から直せる (2) | 4 | 4 | 3 | 4 | 4 | 4 |
| R3 upstream 互換と fork 差分の小ささ (2) | 5 | 3 | 1 | 2 | 3 | 5 |
| R4 表現力: 呼び出し側への契約 (2) | 4 | 4 | 5 | 5 | 3 | 2 |
| R5 決定可能な範囲に収まりやすい (2) | 4 | 4 | 3 | 4 | 5 | 4 |
| R6 Gleam らしさ (小さい言語、やり方は一つ) (1) | 3 | 3 | 2 | 4 | 5 | 5 |
| **合計** | **46** | **42** | **32** | **49** | **50** | **44** |

主な理由:

- P3 はシグネチャが読めなくなり、引数同士の参照で条件が重くなりやすい。差分も最大。
- P2 は差分は小さいが upstream で壊れるので、P1 より得るものがない。
- P5 は Gleam の opaque type にそのまま乗る。証明義務がモジュール内の構築箇所に限られ、E2 の通り軽い。表現できるのはデータの不変条件だけ。
- P6 は導入コストゼロで S5 に直接効くが、E3 の制約 1 により契約の代わりにはならない。
- P4 は人間のレビューに一番向くが、パーサと型検査の両方に手が入る。

## 5. 推奨

段階で組み合わせる。一つに絞らない理由は、P5/P6 と P4 が担う役割 (データ不変条件、部分関数、関数契約) が重ならないため。

1. **MVP**: P6 (既存 `assert` の証明と実行時チェックの除去) + P1 (契約を doc comment で)。どちらも upstream でビルドできるので、実在の Hex パッケージで試せる。仕様の式言語は P4 と同じものにしておく。
2. **次**: P5 (opaque invariant)。構文追加は型宣言の一箇所。
3. **目標構文**: P4。式言語が P1 と同じなので、P1 から P4 への書き換えは機械的にできる。移行後は P1 を廃止し、やり方を一つに戻す。
4. C2 (FFI trust) は MVP から入れる。境界がないと「証明済み」の意味が FFI 経由で崩れる。C1 (protocol) は最後。

## 6. 未解決

- 仕様の中で呼べる関数の範囲。`list.length` のような stdlib 関数を Z3 にどうモデル化するか (純粋かつ全域な関数に限る、など)。
- 型変数 (`List(a)`)、`Float`、`String`、`BitArray` のエンコード。
- package interface (`metadata`) に契約を載せる形式と、契約変更時の互換性判定。
- hot code reload で契約が変わったときの扱い。
- エンコーダの差分テスト (制約 2) を最初のマイルストーンに含めるか。
