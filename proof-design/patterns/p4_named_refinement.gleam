// P4: named refinements + requires/ensures clauses after the signature.

pub type Natural = Int where it >= 0
pub type Positive = Int where it > 0

pub fn withdraw(balance: Natural, amount: Positive) -> Natural
  requires amount <= balance
  ensures result == balance - amount
{
  balance - amount
}

pub fn at(items: List(a), index: Natural) -> a
  requires index < list.length(items)
{
  let assert [first, ..rest] = items
  case index {
    0 -> first
    _ -> at(rest, index - 1)
  }
}
