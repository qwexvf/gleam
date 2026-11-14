// C2: FFI trust boundary (cross-cutting).

// The Erlang side is unverified, so its claim is checked at runtime once,
// at the boundary, and trusted afterwards.
@external(erlang, "bank_store", "fetch")
pub fn fetch(id: String) -> trusted Natural

// `assume` skips the runtime check; the compiler lists every assume in
// `gleam verify --report` so reviewers can audit them.
@external(erlang, "erlang", "system_time")
pub fn now() -> assume Positive
