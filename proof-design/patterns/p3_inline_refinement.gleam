// P3: refinement types written inline in the signature.

pub fn withdraw(
  balance: {b: Int | b >= 0},
  amount: {a: Int | a > 0 && a <= balance},
) -> {r: Int | r == balance - amount && r >= 0} {
  balance - amount
}

pub fn at(items: List(a), index: {i: Int | i >= 0 && i < list.length(items)}) -> a {
  let assert [first, ..rest] = items
  case index {
    0 -> first
    _ -> at(rest, index - 1)
  }
}
