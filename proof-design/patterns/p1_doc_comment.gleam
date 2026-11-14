// P1: specs in doc comments. Upstream gleam compiles this file unchanged;
// only the fork reads the @-lines.

/// Withdraw `amount` from `balance`.
/// @requires balance >= 0
/// @requires amount > 0 && amount <= balance
/// @ensures result == balance - amount && result >= 0
pub fn withdraw(balance: Int, amount: Int) -> Int {
  balance - amount
}

/// @requires index >= 0 && index < list.length(items)
pub fn at(items: List(a), index: Int) -> a {
  let assert [first, ..rest] = items
  case index {
    0 -> first
    _ -> at(rest, index - 1)
  }
}
