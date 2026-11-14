// P6: no new syntax. The existing `assert` and `let assert` become proof
// obligations: proved ones lose their runtime check, unproved ones warn.

pub fn withdraw(balance: Int, amount: Int) -> Int {
  assert balance >= 0          // unprovable here: callers are unknown
  assert amount > 0 && amount <= balance
  let result = balance - amount
  assert result >= 0           // provable from the two asserts above
  result
}

pub fn first_of_non_empty(items: List(Int)) -> Int {
  case items {
    [] -> 0
    _ -> {
      let assert [first, ..] = items   // provable: [] handled above
      first
    }
  }
}
