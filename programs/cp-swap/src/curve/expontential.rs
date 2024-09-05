use anchor_lang::prelude::*;
use std::fmt;

#[account]
#[derive(InitSpace)]
pub struct BondingCurve {
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}

impl BondingCurve {
    pub const SEED_PREFIX: &'static [u8; 13] = b"bonding-curve";
}

impl fmt::Display for BondingCurve {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "virtual_sol_reserves: {}, virtual_token_reserves: {}, real_sol_reserves: {}, real_token_reserves: {}, token_total_supply: {}, complete: {}",
            self.virtual_sol_reserves,
            self.virtual_token_reserves,
            self.real_sol_reserves,
            self.real_token_reserves,
            self.token_total_supply,
            self.complete
        )
    }
}

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
#[zero_copy]
#[derive(Debug)]
pub struct AMM {
    pub virtual_token_0_reserves: u128,
    pub virtual_token_1_reserves: u128,
    pub real_token_0_reserves: u128,
    pub real_token_1_reserves: u128,
    pub initial_virtual_token_reserves: u128,
}

impl Default for AMM {
    fn default() -> Self {
        AMM {
            virtual_token_0_reserves: 1_000_000_000_000_000_000u128,
            virtual_token_1_reserves: 1_000_000_000_000_000_000u128,
            real_token_0_reserves: 1_000_000_000_000u128,
            real_token_1_reserves: 1_000_000_000_000u128,
            initial_virtual_token_reserves: 1_000_000_000_000_000_000u128,
        }
    }
}

impl AMM {
    pub fn new(
        virtual_token_0_reserves: u128,
        virtual_token_1_reserves: u128,
        real_token_0_reserves: u128,
        real_token_1_reserves: u128,
        initial_virtual_token_reserves: u128,
    ) -> Self {
        AMM {
            virtual_token_0_reserves,
            virtual_token_1_reserves,
            real_token_0_reserves,
            real_token_1_reserves,
            initial_virtual_token_reserves,
        }
    }
    pub fn get_buy_price(&self, tokens: u128) -> Option<u128> {
        if tokens == 0 || tokens > self.virtual_token_1_reserves {
            return None;
        }

        let product_of_reserves = self.virtual_token_0_reserves.checked_mul(self.virtual_token_1_reserves)?;
        let new_virtual_token_1_reserves = self.virtual_token_1_reserves.checked_sub(tokens)?;
        let new_virtual_token_0_reserves = product_of_reserves.checked_div(new_virtual_token_1_reserves)?.checked_add(1)?;
        let amount_needed = new_virtual_token_0_reserves.checked_sub(self.virtual_token_0_reserves)?;

        Some(amount_needed)
    }

    pub fn apply_buy(&mut self, token_amount: u128) -> Option<BuyResult> {
        let final_token_amount = token_amount.min(self.real_token_1_reserves);
        let sol_amount = self.get_buy_price(final_token_amount)?;

        self.virtual_token_1_reserves = self.virtual_token_1_reserves.checked_sub(final_token_amount)?;
        self.real_token_1_reserves = self.real_token_1_reserves.checked_sub(final_token_amount)?;

        self.virtual_token_0_reserves = self.virtual_token_0_reserves.checked_add(sol_amount)?;
        self.real_token_0_reserves = self.real_token_0_reserves.checked_add(sol_amount)?;

        Some(BuyResult {
            token_amount: final_token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn get_sell_price(&self, tokens: u128) -> Option<u128> {
        if tokens == 0 || tokens > self.virtual_token_1_reserves {
            return None;
        }

        let product_of_reserves = self.virtual_token_0_reserves.checked_mul(self.virtual_token_1_reserves)?;
        let new_virtual_token_1_reserves = self.virtual_token_1_reserves.checked_add(tokens)?;
        let new_virtual_token_0_reserves = product_of_reserves.checked_div(new_virtual_token_1_reserves)?;
        let amount_received = self.virtual_token_0_reserves.checked_sub(new_virtual_token_0_reserves)?;

        Some(amount_received.min(self.real_token_0_reserves))
    }

    pub fn apply_sell(&mut self, token_amount: u128) -> Option<SellResult> {
        let sol_amount = self.get_sell_price(token_amount)?;

        self.virtual_token_1_reserves = self.virtual_token_1_reserves.checked_add(token_amount)?;
        self.real_token_1_reserves = self.real_token_1_reserves.checked_add(token_amount)?;

        self.virtual_token_0_reserves = self.virtual_token_0_reserves.checked_sub(sol_amount)?;
        self.real_token_0_reserves = self.real_token_0_reserves.checked_sub(sol_amount)?;

        Some(SellResult {
            token_amount: token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn apply_deposit(&mut self, token_0_amount: u128, token_1_amount: u128) -> Result<()> {
        self.virtual_token_0_reserves = self.virtual_token_0_reserves.checked_add(token_0_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.virtual_token_1_reserves = self.virtual_token_1_reserves.checked_add(token_1_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.real_token_0_reserves = self.real_token_0_reserves.checked_add(token_0_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.real_token_1_reserves = self.real_token_1_reserves.checked_add(token_1_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        Ok(())
    }
    pub fn apply_withdraw(&mut self, token_0_amount: u128, token_1_amount: u128) -> Result<()> {
        self.virtual_token_0_reserves = self.virtual_token_0_reserves.checked_sub(token_0_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.virtual_token_1_reserves = self.virtual_token_1_reserves.checked_sub(token_1_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.real_token_0_reserves = self.real_token_0_reserves.checked_sub(token_0_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        self.real_token_1_reserves = self.real_token_1_reserves.checked_sub(token_1_amount).ok_or(ErrorCode::ConstraintMintMintAuthority)?;
        Ok(())
    }

}


impl fmt::Display for AMM {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AMM {{ virtual_token_0_reserves: {}, virtual_token_1_reserves: {}, real_token_0_reserves: {}, real_token_1_reserves: {}, initial_virtual_token_reserves: {} }}",
            self.virtual_token_0_reserves, self.virtual_token_1_reserves, self.real_token_0_reserves, self.real_token_1_reserves, self.initial_virtual_token_reserves
        )
    }
}
