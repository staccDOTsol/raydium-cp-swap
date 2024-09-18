use std::fmt;
use anchor_lang::prelude::*;
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

#[derive(Debug, Clone, Default, Copy, AnchorSerialize, AnchorDeserialize)]
pub struct AMM {
    pub virtual_sol_reserves: u128,
    pub virtual_token_reserves: u128,
    pub real_sol_reserves: u128,
    pub real_token_reserves: u128,
    pub initial_virtual_token_reserves: u128,
}
pub const MAX_TOKEN_SUPPLY: u128 = 42_000;
pub const DEFAULT_DECIMALS: u8 = 9;

pub const DEFAULT_TOKEN_BALANCE: u128 = MAX_TOKEN_SUPPLY * 10u128.pow(DEFAULT_DECIMALS as u32);
pub const DEFAULT_INITIAL_TOKEN_RESERVES: u128 = 793_100_000_000_000 ;
pub const DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE: u128 = 30_000_000_000 ; // Increased by 100x
pub const DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE: u128 = (DEFAULT_TOKEN_BALANCE as f64 * 1.073) as u128 / 10000;
pub const DEFAULT_SOL_BALANCE: u128 = (DEFAULT_TOKEN_BALANCE as f64 * 0.00003 * 100.0) as u128; // Increased by 100x

impl AMM {
    pub fn new(
        virtual_sol_reserves: u128,
        virtual_token_reserves: u128,
        real_sol_reserves: u128,
        real_token_reserves: u128,
        initial_virtual_token_reserves: u128,
    ) -> Self {
        AMM {
            virtual_sol_reserves,
            virtual_token_reserves,
            real_sol_reserves,
            real_token_reserves,
            initial_virtual_token_reserves,
        }
    }
    pub fn update(&mut self, current_supply: u128) -> Result<()> {
        if current_supply > DEFAULT_TOKEN_BALANCE {
            return Err(ErrorCode::AccountReallocExceedsLimit.into());
        }
        msg!("Previous AMM state: {:?}", self.get_state()); // Log the updated state
        // Update real token reserves to match current supply
        self.real_token_reserves = current_supply;
        self.real_token_reserves = DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE;
        self.virtual_sol_reserves = DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE;
        self.virtual_token_reserves = DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE;

        msg!("Updated AMM state: {:?}", self.get_state()); // Log the updated state

        Ok(())
    }

    // Helper method to get current state (useful for testing and verification)
    pub fn get_state(&self) -> (u128, u128, u128, u128, u128) {
        (
            self.virtual_sol_reserves,
            self.virtual_token_reserves,
            self.real_sol_reserves,
            self.real_token_reserves,
            self.initial_virtual_token_reserves,
        )
    }
    pub fn get_buy_price(&self, tokens: u128) -> Option<u128> {
        if tokens == 0 || tokens > DEFAULT_TOKEN_BALANCE.saturating_sub(self.real_token_reserves) {
            return None;
        }

        let product_of_reserves = self.virtual_sol_reserves.checked_mul(self.virtual_token_reserves)?;
        let new_virtual_token_reserves = self.virtual_token_reserves.checked_sub(tokens)?;
        let new_virtual_sol_reserves = product_of_reserves.checked_div(new_virtual_token_reserves)?.checked_add(1)?;
        let amount_needed = new_virtual_sol_reserves.checked_sub(self.virtual_sol_reserves)?;

        msg!("Buy price calculated: {}", amount_needed); // Log the calculated buy price

        Some(amount_needed)
    }

    pub fn apply_buy(&mut self, token_amount: u128) -> Option<BuyResult> {
        let available_tokens = DEFAULT_TOKEN_BALANCE.saturating_sub(self.real_token_reserves);
        let final_token_amount = token_amount.min(available_tokens);

        let sol_amount = self.get_buy_price(final_token_amount)?;

        self.virtual_token_reserves = self.virtual_token_reserves.checked_sub(final_token_amount)?;
        self.real_token_reserves = self.real_token_reserves.checked_add(final_token_amount)?;

        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_add(sol_amount)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_add(sol_amount)?;

        msg!("Applied buy: token_amount: {}, sol_amount: {}", final_token_amount, sol_amount); // Log the buy application

        Some(BuyResult {
            token_amount: final_token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn get_sell_price(&self, tokens: u128) -> Option<u128> {
        if tokens == 0 || tokens > self.real_token_reserves {
            msg!("Attempted to get sell price for invalid token amount: {}", tokens);
            return None;
        }

        let scaling_factor = self.initial_virtual_token_reserves;

        let scaled_tokens = tokens.checked_mul(scaling_factor)?;
        let token_sell_proportion = scaled_tokens.checked_div(self.virtual_token_reserves)?;
        let sol_received = (self.virtual_sol_reserves.checked_mul(token_sell_proportion)?).checked_div(scaling_factor)?;

        msg!("Calculated sell price for {} tokens: {}", tokens, sol_received); // Improved logging for calculated sell price

        Some(sol_received.min(self.real_sol_reserves))
    }

    pub fn apply_sell(&mut self, token_amount: u128) -> Option<SellResult> {
        if token_amount > self.real_token_reserves {
            msg!("Sell attempt exceeds available real token reserves: requested {}, available {}", token_amount, self.real_token_reserves);
            return None;
        }

        self.virtual_token_reserves = self.virtual_token_reserves.checked_add(token_amount)?;
        self.real_token_reserves = self.real_token_reserves.checked_sub(token_amount)?;

        let sol_amount = self.get_sell_price(token_amount)?;

        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_sub(sol_amount)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_sub(sol_amount)?;

        msg!("Sell executed successfully: token_amount: {}, sol_amount: {}", token_amount, sol_amount); // Improved logging for sell application

        Some(SellResult {
            token_amount: token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }
}

impl fmt::Display for AMM {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AMM {{ virtual_sol_reserves: {}, virtual_token_reserves: {}, real_sol_reserves: {}, real_token_reserves: {}, initial_virtual_token_reserves: {}, max_token_supply: {} }}",
            self.virtual_sol_reserves, self.virtual_token_reserves, self.real_sol_reserves, self.real_token_reserves, self.initial_virtual_token_reserves, DEFAULT_TOKEN_BALANCE
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::curve::expontential::AMM;
    use super::DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE;
    use super::DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE;
    use super::DEFAULT_SOL_BALANCE;

    #[test]
    fn test_buy_and_sell_too_much() {
        let mut amm = AMM::new(
            DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
            DEFAULT_SOL_BALANCE,
            0,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
        );

        // Attempt to buy more tokens than available in reserves
        let buy_result = amm.apply_buy(2000);
        assert!(buy_result.is_some(), "Buy should succeed");
        let buy_result = buy_result.unwrap();
        println!("Buy Result: {:?}", buy_result);
        println!("AMM after buy: {}", amm);

        // Attempt to sell more tokens than available in reserves
        let sell_result = amm.apply_sell(2001);
        assert!(sell_result.is_none(), "Sell should fail");
        println!("Sell Result: {:?}", sell_result);
        println!("AMM after sell: {}", amm);
    }

    #[test]
    fn test_apply_sell() {
        let mut amm = AMM::new(
            DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
            DEFAULT_SOL_BALANCE,
            1000,  // Set initial real_token_reserves
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
        );
        let result = amm.apply_sell(100);
        assert!(result.is_some(), "Sell should succeed");
        let result = result.unwrap();

        println!("Sell Result: {:?}", result);
        println!("AMM after sell: {}", amm);
    }

    #[test]
    fn test_get_sell_price() {
        let amm = AMM::new(
            DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
            DEFAULT_SOL_BALANCE,
            1000,  // Set initial real_token_reserves
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
        );

        println!("Sell price for 0 tokens: {:?}", amm.get_sell_price(0));
        println!("Sell price for 100 tokens: {:?}", amm.get_sell_price(100));
        println!("Sell price for 5000 tokens: {:?}", amm.get_sell_price(5000));
    }

    #[test]
    fn test_apply_buy() {
        let mut amm = AMM::new(
            DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
            DEFAULT_SOL_BALANCE,
            0,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
        );

        let purchase_amount = 100;

        let result = amm.apply_buy(purchase_amount).unwrap();
        
        println!("Buy Result: {:?}", result);
        println!("AMM after buy: {}", amm);
    }

    #[test]
    fn test_get_buy_price() {
        let amm = AMM::new(
            DEFAULT_INITIAL_VIRTUAL_SOL_RESERVE,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
            DEFAULT_SOL_BALANCE,
            0,
            DEFAULT_INITIAL_VIRTUAL_TOKEN_RESERVE,
        );
        
        println!("Buy price for 0 tokens: {:?}", amm.get_buy_price(0));
        println!("Buy price for 100 tokens: {:?}", amm.get_buy_price(100));
        println!("Buy price for 2000 tokens: {:?}", amm.get_buy_price(2000));
    }
}