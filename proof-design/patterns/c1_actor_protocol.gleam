// C1: message protocol for an actor (cross-cutting, combines with any P).

pub protocol Account {
  Withdraw(amount: Positive) -> reply Result(Natural, Insufficient)
  Balance -> reply Natural
  Close -> end
}

// The checker rejects a handler that misses a message, replies with the
// wrong type, or keeps serving after Close.
