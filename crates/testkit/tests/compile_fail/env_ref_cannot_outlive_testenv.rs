// Holding a `&Env` borrowed from `TestEnv::env()` across a `reset()` must
// not compile.
//
// `env()` lends a shared reference tied to the lifetime of the borrow of
// `self`, and `reset()` takes `&mut self`, so the borrow checker rejects
// using the escape-hatch reference after a reset — a live reference can
// never observe (or block) the reset of the environment it came from.
//
// See: <https://github.com/soroban-testkit/soroban-testkit/issues/32>

fn main() {
    let mut testkit_env = soroban_testkit::core::TestEnv::new();
    let sdk_env = testkit_env.env(); // &Env — lifetime tied to testkit_env
    // This must fail: `reset` requires `&mut self` but `testkit_env` is
    // immutably borrowed by `sdk_env`.
    testkit_env.reset(); // ERROR: cannot borrow as mutable while `sdk_env` is live
    let _ = sdk_env; // extend the escape-hatch borrow past the reset call
}
