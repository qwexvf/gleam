// P5: invariants attached to opaque types. Only code inside this module can
// build a Balance, so only those sites carry a proof obligation; every other
// module gets `value >= 0` for free.

pub opaque type Balance {
  Balance(value: Int)
} invariant { value >= 0 }

pub fn new(value: Int) -> Result(Balance, Nil) {
  case value >= 0 {
    True -> Ok(Balance(value))   // proved: guarded by the case
    False -> Error(Nil)
  }
}

pub fn deposit(balance: Balance, amount: Int) -> Balance
  requires amount > 0
{
  Balance(balance.value + amount)   // proved: invariant + requires
}

pub fn withdraw(balance: Balance, amount: Int) -> Result(Balance, Nil) {
  case amount > 0 && amount <= balance.value {
    True -> Ok(Balance(balance.value - amount))
    False -> Error(Nil)
  }
}
