use anchor_lang::prelude::*;
use std::fmt;

#[derive(Debug)]
pub struct BuyResult {
    pub token_amount: u64,
    pub sol_amount: u64,
}

#[derive(Debug)]
pub struct SellResult {
    pub token_amount: u64,
    pub sol_amount: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct AMM {
    pub virtual_sol_reserves: u128,
    pub virtual_token_reserves: u128,
    pub real_sol_reserves: u128,
    pub real_token_reserves: u128,
    pub initial_virtual_token_reserves: u128,
}

impl Default for AMM {
    fn default() -> Self {
        AMM::new()
    }
}

impl AMM {
    pub fn new() -> Self {
        AMM {
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_073_000_000_000_000,
            real_sol_reserves: 1_000_000,
            real_token_reserves: 793_100_000_000_000,
            initial_virtual_token_reserves: 1_073_000_000_000_000,
        }
    }

    pub fn get_buy_price(&self, tokens: u128) -> Option<u128> {
        if tokens == 0 || tokens > self.virtual_token_reserves {
            return None;
        }

        let product_of_reserves = self.virtual_sol_reserves.checked_mul(self.virtual_token_reserves)?;
        let new_virtual_token_reserves = self.virtual_token_reserves.checked_sub(tokens)?;
        let new_virtual_sol_reserves = product_of_reserves.checked_div(new_virtual_token_reserves)?.checked_add(1)?;
        let amount_needed = new_virtual_sol_reserves.checked_sub(self.virtual_sol_reserves)?;

        Some(amount_needed)
    }

    pub fn apply_buy(&mut self, token_amount: u128) -> Option<BuyResult> {
        let final_token_amount = if token_amount > self.real_token_reserves {
            self.real_token_reserves
        } else {
            token_amount
        };

        let sol_amount = self.get_buy_price(final_token_amount)?;

        self.virtual_token_reserves = self.virtual_token_reserves.checked_sub(final_token_amount)?;
        self.real_token_reserves = self.real_token_reserves.checked_sub(final_token_amount)?;

        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_add(sol_amount)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_add(sol_amount)?;

        Some(BuyResult {
            token_amount: final_token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn apply_sell(&mut self, token_amount: u128) -> Option<SellResult> {
        self.virtual_token_reserves = self.virtual_token_reserves.checked_add(token_amount)?;
        self.real_token_reserves = self.real_token_reserves.checked_add(token_amount)?;

        let sol_amount = self.get_sell_price(token_amount)?;

        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_sub(sol_amount)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_sub(sol_amount)?;

        Some(SellResult {
            token_amount: token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn get_sell_price(&self, tokens: u128) -> Option<u128> {
        if tokens <= 0 || tokens > self.virtual_token_reserves {
            return None;
        }

        let scaling_factor = self.initial_virtual_token_reserves;

        let scaled_tokens = tokens.checked_mul(scaling_factor)?;
        let token_sell_proportion = scaled_tokens.checked_div(self.virtual_token_reserves)?;
        let sol_received = (self.virtual_sol_reserves.checked_mul(token_sell_proportion)?).checked_div(scaling_factor)?;

        Some(sol_received.min(self.real_sol_reserves))
    }
}


impl fmt::Display for AMM {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AMM {{ virtual_sol_reserves: {}, virtual_token_reserves: {}, real_sol_reserves: {}, real_token_reserves: {}, initial_virtual_token_reserves: {} }}",
            self.virtual_sol_reserves, self.virtual_token_reserves, self.real_sol_reserves, self.real_token_reserves, self.initial_virtual_token_reserves
        )
    }
}
