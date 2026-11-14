import gleam/int
import gleam/io

// 1. correct: proved
@requires(balance >= 0)
@requires(amount > 0 && amount <= balance)
@ensures(result >= 0)
pub fn withdraw(balance: Int, amount: Int) -> Int {
  balance - amount
}

// 2. implementation bug: fee makes ensures false
@requires(balance >= 0)
@requires(amount > 0 && amount <= balance)
@ensures(result >= 0)
pub fn withdraw_with_fee(balance: Int, amount: Int) -> Int {
  balance - amount - 1
}

// 3. caller guards the call: proved
@requires(balance >= 0)
@ensures(result >= 0)
pub fn safe_withdraw(balance: Int, amount: Int) -> Int {
  case amount > 0 && amount <= balance {
    True -> withdraw(balance, amount)
    False -> balance
  }
}

// 4. caller forgets the guard: requires of withdraw violated
pub fn careless(balance: Int) -> Int {
  withdraw(balance, 10)
}

// 5. Gleam division semantics: true in Gleam, false with Euclidean div
@requires(n < 0)
@ensures(result * 2 >= n)
pub fn half(n: Int) -> Int {
  n / 2
}

// 6. recursion: modular, uses its own ensures for the recursive call
@requires(n >= 0)
@ensures(result >= n)
pub fn sum_to(n: Int) -> Int {
  case n {
    0 -> 0
    _ -> n + sum_to(n - 1)
  }
}

pub fn main() {
  let requires = 3
  io.println(int.to_string(withdraw(10, requires)))
  io.println(int.to_string(sum_to(4)))
}

// negative controls: each must produce a counterexample
@requires(n < 0)
@ensures(result * 2 == n)
pub fn half_exact(n: Int) -> Int {
  n / 2
}

@requires(n >= 0)
@ensures(result > n)
pub fn sum_to_strict(n: Int) -> Int {
  case n {
    0 -> 0
    _ -> n + sum_to_strict(n - 1)
  }
}

@requires(b != 0)
@ensures(result >= 0)
pub fn rem_sign(a: Int, b: Int) -> Int {
  a % b
}
