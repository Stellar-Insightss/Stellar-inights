#![no_std]

mod storage;

use soroban_sdk::{contract, contracterror, contractimpl, token, Address, Env};
use storage::{bump_instance, load_offer, next_offer_id, save_offer, Offer};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum Error {
    OfferNotFound = 1,
    AlreadyFilled = 2,
    Cancelled = 3,
    /// `output_amount` provided by the settler is below the maker's `min_output`.
    SlippageExceeded = 4,
    NotMaker = 5,
    InvalidAmount = 6,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct TokenSwapContract;

#[contractimpl]
impl TokenSwapContract {
    // -----------------------------------------------------------------------
    // Maker side
    // -----------------------------------------------------------------------

    /// Create an offer.
    ///
    /// The maker deposits `amount_in` of `token_in` into this contract at
    /// offer creation time.  `min_output` is the slippage guard – the maker
    /// will not accept fewer than this many units of `token_out`.
    pub fn create_offer(
        env: Env,
        maker: Address,
        token_in: Address,
        amount_in: i128,
        token_out: Address,
        min_output: i128,
    ) -> Result<u64, Error> {
        if amount_in <= 0 || min_output <= 0 {
            return Err(Error::InvalidAmount);
        }
        maker.require_auth();

        // Transfer `amount_in` from maker into the contract escrow.
        let contract_id = env.current_contract_address();
        token::Client::new(&env, &token_in).transfer(&maker, &contract_id, &amount_in);

        let id = next_offer_id(&env, &maker);
        let offer = Offer {
            id,
            maker: maker.clone(),
            token_in,
            amount_in,
            token_out,
            min_output,
            filled: false,
            cancelled: false,
        };
        save_offer(&env, &offer);
        bump_instance(&env);
        Ok(id)
    }

    /// Cancel an open offer and refund the escrowed `token_in`.
    pub fn cancel_offer(env: Env, maker: Address, offer_id: u64) -> Result<(), Error> {
        maker.require_auth();
        let mut offer = load_offer(&env, maker.clone(), offer_id);

        if offer.filled {
            return Err(Error::AlreadyFilled);
        }
        if offer.cancelled {
            return Err(Error::Cancelled);
        }

        offer.cancelled = true;
        save_offer(&env, &offer);

        // Refund escrowed tokens.
        let contract_id = env.current_contract_address();
        token::Client::new(&env, &offer.token_in).transfer(
            &contract_id,
            &maker,
            &offer.amount_in,
        );

        bump_instance(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Taker side
    // -----------------------------------------------------------------------

    /// Settle an offer.
    ///
    /// The settler provides `output_amount` of `token_out`.
    /// The transaction reverts if `output_amount < offer.min_output` –
    /// **this is the slippage / front-running guard**: even if a miner
    /// re-orders the settle tx after a price-moving tx, the maker's floor
    /// is enforced at execution time.
    ///
    /// On success:
    ///   - settler sends `output_amount` of `token_out` → maker
    ///   - contract releases `amount_in` of `token_in` → settler
    pub fn settle_offer(
        env: Env,
        settler: Address,
        maker: Address,
        offer_id: u64,
        output_amount: i128,
    ) -> Result<(), Error> {
        settler.require_auth();
        let mut offer = load_offer(&env, maker.clone(), offer_id);

        if offer.filled {
            return Err(Error::AlreadyFilled);
        }
        if offer.cancelled {
            return Err(Error::Cancelled);
        }
        // Slippage protection: reject if settler offers less than the minimum.
        if output_amount < offer.min_output {
            return Err(Error::SlippageExceeded);
        }

        offer.filled = true;
        save_offer(&env, &offer);

        let contract_id = env.current_contract_address();

        // settler → maker: `token_out`
        token::Client::new(&env, &offer.token_out).transfer(
            &settler,
            &offer.maker,
            &output_amount,
        );

        // contract → settler: `token_in` (previously escrowed)
        token::Client::new(&env, &offer.token_in).transfer(
            &contract_id,
            &settler,
            &offer.amount_in,
        );

        bump_instance(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Getters
    // -----------------------------------------------------------------------

    pub fn get_offer(env: Env, maker: Address, offer_id: u64) -> Offer {
        load_offer(&env, maker, offer_id)
    }
}
