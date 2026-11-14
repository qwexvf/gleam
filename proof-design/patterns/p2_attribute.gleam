// P2: new attributes. Upstream gleam rejects them (UnknownAttribute,
// compiler-core/src/parse.rs:4872), so the file only builds on the fork.

@requires(balance >= 0)
@requires(amount > 0 && amount <= balance)
@ensures(result == balance - amount && result >= 0)
pub fn withdraw(balance: Int, amount: Int) -> Int {
  balance - amount
}

@requires(index >= 0 && index < list.length(items))
@decreases(index)
pub fn at(items: List(a), index: Int) -> a {
  let assert [first, ..rest] = items
  case index {
    0 -> first
    _ -> at(rest, index - 1)
  }
}
