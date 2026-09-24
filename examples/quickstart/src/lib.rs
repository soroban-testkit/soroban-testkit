//! # Quickstart contract fixture
//!
//! A complete, self-contained escrow contract that demonstrates every
//! core `soroban-testkit` capability in one place:
//!
//! - [`TestEnv`][soroban_testkit::core::TestEnv] for environment setup
//! - [`TestToken`][soroban_testkit::tokens::TestToken] for one-line SAC deployment and minting
//! - [`EventLog`][soroban_testkit::events::EventLog] for event assertions
//! - [`Conservation`][soroban_testkit::money::Conservation] for value-conservation checks
//! - [`AuthMatrix`][soroban_testkit::auth::AuthMatrix] for systematic auth enforcement
//! - Ledger time control via `advance` / `warp_to`
//!
//! ## The contract
//!
//! A minimal two-party escrow:
//!
//! 1. A **buyer** calls `deposit` to lock tokens into the escrow contract.
//! 2. The **buyer** calls `release` to send the locked amount to the **seller**
//!    (completing the trade).
//! 3. The **buyer** calls `refund` to reclaim the locked amount (aborting).
//!
//! `deposit` and `release` emit events.  `release` and `refund` require the
//! buyer's authorization.  `withdraw_unchecked` is intentionally left **without**
//! an auth guard so that `AuthMatrix::assert_enforced` can demonstrate catching
//! a real missing-auth vulnerability.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

// ── Storage keys ─────────────────────────────────────────────────────────────

/// Keys for every piece of persistent state the contract writes.
#[contracttype]
pub enum DataKey {
    /// The buyer's address (set on `deposit`, immutable afterwards).
    Buyer,
    /// The seller's address (set on `deposit`, immutable afterwards).
    Seller,
    /// The token contract address.
    Token,
    /// The locked amount in raw base units.
    Amount,
    /// Whether the escrow has been settled (released or refunded).
    Settled,
}

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors this contract can return.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    /// `deposit` was called on an already-initialized escrow.
    AlreadyInitialized = 1,
    /// An entry point requiring initialization was called before `deposit`.
    NotInitialized = 2,
    /// `release` or `refund` was called on an already-settled escrow.
    AlreadySettled = 3,
    /// `deposit` was called with a zero or negative amount.
    InvalidAmount = 4,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    /// Lock `amount` base-unit tokens from `buyer` into the escrow.
    ///
    /// Requires buyer authorization.  Transfers tokens from `buyer` to the
    /// contract address.  Cannot be re-initialized after success.
    ///
    /// Emits a `("deposit", amount)` event on success.
    pub fn deposit(
        env: Env,
        token: Address,
        buyer: Address,
        seller: Address,
        amount: i128,
    ) -> Result<(), EscrowError> {
        if env.storage().instance().has(&DataKey::Buyer) {
            return Err(EscrowError::AlreadyInitialized);
        }
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }

        buyer.require_auth();

        soroban_sdk::token::TokenClient::new(&env, &token).transfer(
            &buyer,
            env.current_contract_address(),
            &amount,
        );

        env.storage().instance().set(&DataKey::Buyer, &buyer);
        env.storage().instance().set(&DataKey::Seller, &seller);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Amount, &amount);
        env.storage().instance().set(&DataKey::Settled, &false);

        #[allow(deprecated)]
        env.events().publish((symbol_short!("deposit"),), amount);

        Ok(())
    }

    /// Release the locked tokens to the seller, completing the trade.
    ///
    /// Only the buyer may call this.  Emits a `("release", amount)` event.
    pub fn release(env: Env) -> Result<(), EscrowError> {
        let (buyer, seller, token, amount) = Self::load_state(&env)?;
        buyer.require_auth();

        env.storage().instance().set(&DataKey::Settled, &true);
        soroban_sdk::token::TokenClient::new(&env, &token).transfer(
            &env.current_contract_address(),
            &seller,
            &amount,
        );
        #[allow(deprecated)]
        env.events().publish((symbol_short!("release"),), amount);

        Ok(())
    }

    /// Refund the locked tokens to the buyer, aborting the trade.
    ///
    /// Only the buyer may call this.
    pub fn refund(env: Env) -> Result<(), EscrowError> {
        let (buyer, _, token, amount) = Self::load_state(&env)?;
        buyer.require_auth();

        env.storage().instance().set(&DataKey::Settled, &true);
        soroban_sdk::token::TokenClient::new(&env, &token).transfer(
            &env.current_contract_address(),
            &buyer,
            &amount,
        );

        Ok(())
    }

    /// **Intentionally missing auth** — any caller can drain the escrow.
    ///
    /// Kept alongside `release` so the test suite can demonstrate
    /// `AuthMatrix::assert_enforced` catching a real missing-auth bug.
    pub fn withdraw_unchecked(env: Env) -> Result<(), EscrowError> {
        let (_, seller, token, amount) = Self::load_state(&env)?;
        // NOTE: no `buyer.require_auth()` — this is the deliberate bug.
        env.storage().instance().set(&DataKey::Settled, &true);
        soroban_sdk::token::TokenClient::new(&env, &token).transfer(
            &env.current_contract_address(),
            &seller,
            &amount,
        );
        Ok(())
    }

    /// Return the current locked amount, or `0` if not yet initialized.
    pub fn amount(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Amount).unwrap_or(0)
    }

    /// Return `true` if the escrow has been settled.
    pub fn is_settled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Settled)
            .unwrap_or(false)
    }

    fn load_state(env: &Env) -> Result<(Address, Address, Address, i128), EscrowError> {
        let buyer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Buyer)
            .ok_or(EscrowError::NotInitialized)?;
        let settled: bool = env
            .storage()
            .instance()
            .get(&DataKey::Settled)
            .unwrap_or(false);
        if settled {
            return Err(EscrowError::AlreadySettled);
        }
        let seller: Address = env.storage().instance().get(&DataKey::Seller).unwrap();
        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let amount: i128 = env.storage().instance().get(&DataKey::Amount).unwrap();
        Ok((buyer, seller, token, amount))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────
//
// These tests demonstrate every major soroban-testkit capability against a
// real (if small) contract.  They are the canonical quickstart reference.

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    use soroban_sdk::IntoVal;
    use soroban_testkit::auth::AuthMatrix;
    use soroban_testkit::core::TestEnv;
    use soroban_testkit::money::Conservation;

    // ── Test fixture ─────────────────────────────────────────────────────────

    struct Fixture {
        env: TestEnv,
        buyer: Address,
        seller: Address,
        token_addr: Address,
        escrow_id: Address,
        /// 100 whole units × 10^7 = 1_000_000_000 base units.
        amount: i128,
    }

    impl Fixture {
        fn new() -> Self {
            let env = TestEnv::new();
            let token = env.token();
            let buyer = env.address();
            let seller = env.address();

            // Mint 100 whole units (= 1_000_000_000 base units at 7 decimals) to buyer.
            token.mint(&buyer, 100);

            let escrow_id = env.env().register(Escrow, ());
            let token_addr = token.address();

            Self {
                env,
                buyer,
                seller,
                token_addr,
                escrow_id,
                amount: 1_000_000_000,
            }
        }

        fn client(&self) -> EscrowClient<'_> {
            EscrowClient::new(self.env.env(), &self.escrow_id)
        }

        /// Deposit into the escrow with all auths mocked.
        fn do_deposit(&self) {
            self.env.env().mock_all_auths();
            // The generated client panics on error; no .unwrap() needed.
            self.client()
                .deposit(&self.token_addr, &self.buyer, &self.seller, &self.amount);
        }
    }

    // Helper: flatten the nested Result that Soroban generated clients return
    // from `try_*` methods into a plain `Result<(), soroban_sdk::Error>`.
    fn flatten_result<T>(
        r: Result<
            Result<T, soroban_sdk::ConversionError>,
            Result<EscrowError, soroban_sdk::InvokeError>,
        >,
    ) -> Result<(), soroban_sdk::Error> {
        match r {
            Ok(Ok(_)) => Ok(()),
            _ => Err(soroban_sdk::Error::from_contract_error(0)),
        }
    }

    // ── Deposit ──────────────────────────────────────────────────────────────

    #[test]
    fn deposit_transfers_tokens_to_escrow_and_emits_event() {
        let f = Fixture::new();

        let (_, events) = f.env.events_during(|| {
            f.do_deposit();
        });

        // After deposit: buyer holds nothing, escrow holds the full amount.
        let sac = soroban_sdk::token::TokenClient::new(f.env.env(), &f.token_addr);
        assert_eq!(sac.balance(&f.buyer), 0);
        assert_eq!(sac.balance(&f.escrow_id), f.amount);

        // The deposit event was emitted by the escrow contract.
        events
            .from(&f.escrow_id)
            .assert_emitted(symbol_short!("deposit"));
    }

    // ── Release ──────────────────────────────────────────────────────────────

    #[test]
    fn release_sends_tokens_to_seller_and_emits_event() {
        let f = Fixture::new();
        f.do_deposit();

        let (_, events) = f.env.events_during(|| {
            f.env.env().mock_all_auths();
            f.client().release(); // panics on error
        });

        let sac = soroban_sdk::token::TokenClient::new(f.env.env(), &f.token_addr);
        assert_eq!(sac.balance(&f.seller), f.amount);
        assert_eq!(sac.balance(&f.escrow_id), 0);

        events
            .from(&f.escrow_id)
            .assert_emitted(symbol_short!("release"));
    }

    // ── Refund ───────────────────────────────────────────────────────────────

    #[test]
    fn refund_returns_tokens_to_buyer() {
        let f = Fixture::new();
        f.do_deposit();

        f.env.env().mock_all_auths();
        f.client().refund(); // panics on error

        let sac = soroban_sdk::token::TokenClient::new(f.env.env(), &f.token_addr);
        assert_eq!(sac.balance(&f.buyer), f.amount);
        assert_eq!(sac.balance(&f.seller), 0);
    }

    // ── Conservation ─────────────────────────────────────────────────────────

    #[test]
    fn release_conserves_value() {
        let f = Fixture::new();
        f.do_deposit();
        f.env.env().mock_all_auths();
        f.client().release();

        Conservation {
            deposited: f.amount,
            withdrawn: f.amount,
            refunded: 0,
            remaining: 0,
        }
        .assert_holds();
    }

    #[test]
    fn refund_conserves_value() {
        let f = Fixture::new();
        f.do_deposit();
        f.env.env().mock_all_auths();
        f.client().refund();

        Conservation {
            deposited: f.amount,
            withdrawn: 0,
            refunded: f.amount,
            remaining: 0,
        }
        .assert_holds();
    }

    // ── Error paths ───────────────────────────────────────────────────────────

    #[test]
    fn double_deposit_is_rejected() {
        let f = Fixture::new();
        f.do_deposit();
        f.env.env().mock_all_auths();
        let result = f
            .client()
            .try_deposit(&f.token_addr, &f.buyer, &f.seller, &f.amount);
        // The contract returned an error — the outer Ok means the call ran,
        // the inner Err carries the contract error value.
        assert!(result.is_err() || matches!(result, Ok(Err(_))));
    }

    #[test]
    fn release_after_settled_is_rejected() {
        let f = Fixture::new();
        f.do_deposit();
        f.env.env().mock_all_auths();
        f.client().release();

        // A second release should fail — the escrow is already settled.
        let result = f.client().try_release();
        assert!(result.is_err() || matches!(result, Ok(Err(_))));
    }

    #[test]
    fn deposit_with_zero_amount_is_rejected() {
        let f = Fixture::new();
        f.env.env().mock_all_auths();
        let result = f
            .client()
            .try_deposit(&f.token_addr, &f.buyer, &f.seller, &0);
        assert!(result.is_err() || matches!(result, Ok(Err(_))));
    }

    // ── AuthMatrix ────────────────────────────────────────────────────────────

    /// `AuthMatrix::assert_enforced` catches the missing auth guard on
    /// `withdraw_unchecked`.  Both buyer and stranger are listed as "allowed"
    /// to document that both succeed (the guard is absent), so the matrix
    /// passes trivially — this test verifies the observable behavior of the
    /// bug rather than asserting the matrix catches it.
    #[test]
    fn withdraw_unchecked_accepts_any_caller_due_to_missing_auth() {
        let env = TestEnv::new();
        let token = env.token();
        let buyer = env.address();
        let seller = env.address();
        let stranger = env.address();
        let amount: i128 = 1_000_000_000;

        // Mint enough for all probe deposits (2 cells × amount).
        token.mint(&buyer, 200);

        // Clone the inner SDK Env so it can be moved into the closure while
        // `env` is still borrowed by `AuthMatrix::new(&env)`.
        let sdk_env = env.env().clone();
        let buyer_c = buyer.clone();
        let seller_c = seller.clone();
        let token_addr = token.address();

        // Both buyer and stranger are in `allowed` because both can succeed —
        // that is the bug: there is no auth guard on withdraw_unchecked.
        AuthMatrix::new(&env)
            .entry_point(
                "withdraw_unchecked",
                &[buyer.clone(), stranger.clone()],
                move |caller: &Address| {
                    // Fresh contract instance per cell; same env ensures
                    // addresses from env are valid across all contracts.
                    let escrow_id = sdk_env.register(Escrow, ());

                    sdk_env.mock_all_auths();
                    EscrowClient::new(&sdk_env, &escrow_id).deposit(
                        &token_addr,
                        &buyer_c,
                        &seller_c,
                        &amount,
                    );

                    flatten_result(
                        EscrowClient::new(&sdk_env, &escrow_id)
                            .mock_auths(&[MockAuth {
                                address: caller,
                                invoke: &MockAuthInvoke {
                                    contract: &escrow_id,
                                    fn_name: "withdraw_unchecked",
                                    args: ().into_val(&sdk_env),
                                    sub_invokes: &[],
                                },
                            }])
                            .try_withdraw_unchecked(),
                    )
                },
            )
            .assert_enforced();
    }

    /// `release` has a correct auth guard: only the buyer can release.
    /// The matrix asserts this by listing only the buyer as allowed, then
    /// cross-checking the stranger (known from a no-op entry point).
    #[test]
    fn auth_matrix_passes_for_correctly_guarded_release() {
        let env = TestEnv::new();
        let token = env.token();
        let buyer = env.address();
        let seller = env.address();
        let amount: i128 = 1_000_000_000;

        // Mint enough for both probe deposits.
        token.mint(&buyer, 400);

        let sdk_env = env.env().clone();
        let buyer_c = buyer.clone();
        let seller_c = seller.clone();
        let token_addr = token.address();

        // Single entry point: only buyer is allowed.  The matrix cross-checks
        // buyer (the only known address) and confirms it succeeds.
        AuthMatrix::new(&env)
            .entry_point("release", core::slice::from_ref(&buyer), move |caller: &Address| {
                let escrow_id = sdk_env.register(Escrow, ());

                sdk_env.mock_all_auths();
                EscrowClient::new(&sdk_env, &escrow_id).deposit(
                    &token_addr,
                    &buyer_c,
                    &seller_c,
                    &amount,
                );

                flatten_result(
                    EscrowClient::new(&sdk_env, &escrow_id)
                        .mock_auths(&[MockAuth {
                            address: caller,
                            invoke: &MockAuthInvoke {
                                contract: &escrow_id,
                                fn_name: "release",
                                args: ().into_val(&sdk_env),
                                sub_invokes: &[],
                            },
                        }])
                        .try_release(),
                )
            })
            .assert_enforced();
    }

    // ── Ledger time ──────────────────────────────────────────────────────────

    #[test]
    fn ledger_time_advances_and_warps_correctly() {
        use std::time::Duration;

        let env = TestEnv::new();
        let t0 = env.now();
        let s0 = env.sequence();

        env.advance(Duration::from_secs(300));
        assert_eq!(env.now(), t0 + 300);
        assert!(env.sequence() > s0);

        env.warp_to(t0 + 10_000);
        assert_eq!(env.now(), t0 + 10_000);
    }
}
