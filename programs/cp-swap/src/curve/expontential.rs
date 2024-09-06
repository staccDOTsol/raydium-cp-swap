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

    pub fn get_buy_price(&mut self, tokens: u128) -> Option<u128> {
        msg!("get_buy_price called with tokens: {}", tokens);

        // Check if tokens exceed virtual_token_reserves and adjust if necessary
        if tokens > self.virtual_token_reserves {
            self.virtual_token_reserves = (self.virtual_token_reserves as f64 * 1.1) as u128;
            msg!("Adjusted virtual_token_reserves to: {}", self.virtual_token_reserves);
        }
        if tokens == 0 {
            msg!("Tokens is 0, returning None");
            return None;
        }

        let product_of_reserves = self.virtual_sol_reserves.checked_mul(self.virtual_token_reserves)?;
        msg!("Product of reserves: {}", product_of_reserves);

        let new_virtual_token_reserves = self.virtual_token_reserves.checked_sub(tokens)?;
        msg!("New virtual token reserves: {}", new_virtual_token_reserves);

        let new_virtual_sol_reserves = product_of_reserves.checked_div(new_virtual_token_reserves)?.checked_add(1)?;
        msg!("New virtual SOL reserves: {}", new_virtual_sol_reserves);
        let amount_needed = new_virtual_sol_reserves.checked_sub(self.virtual_sol_reserves)?;

        msg!("Amount needed: {}", amount_needed);
        // Update virtual and real reserves
        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_add(amount_needed)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_add(amount_needed)?;
        let amount_needed = amount_needed ;
        msg!("Amount needed: {}", amount_needed);


        msg!("Updated virtual_sol_reserves: {}", self.virtual_sol_reserves);
        msg!("Updated real_sol_reserves: {}", self.real_sol_reserves);
        msg!("Updated virtual_token_reserves: {}", self.virtual_token_reserves);
        msg!("Updated real_token_reserves: {}", self.real_token_reserves);

        msg!("Returning amount_needed: {}", amount_needed);
        Some(amount_needed)
    }

    pub fn apply_buy(&mut self, token_amount: u128) -> Option<BuyResult> {
        let final_token_amount = token_amount.min(self.real_token_reserves);
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
        let sol_amount = self.get_sell_price(token_amount)?;

        self.virtual_token_reserves = self.virtual_token_reserves.checked_add(token_amount)?;
        self.real_token_reserves = self.real_token_reserves.checked_add(token_amount)?;
        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_sub(sol_amount)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_sub(sol_amount)?;

        Some(SellResult {
            token_amount: token_amount as u64,
            sol_amount: sol_amount as u64,
        })
    }

    pub fn get_sell_price(&mut self, tokens: u128) -> Option<u128> {
         if tokens > self.virtual_token_reserves {
            self.virtual_token_reserves = (self.virtual_token_reserves as f64 * 1.1) as u128;
        }
        if tokens == 0 || tokens > self.virtual_token_reserves {
            return None;
        }

        let scaling_factor = self.initial_virtual_token_reserves;
        let scaled_tokens = tokens.checked_mul(scaling_factor)?;
        let token_sell_proportion = scaled_tokens.checked_div(self.virtual_token_reserves)?;
        let sol_received = (self.virtual_sol_reserves.checked_mul(token_sell_proportion)?).checked_div(scaling_factor)?;
        // Calculate the amount of SOL to be received
        let sol_to_receive = (self.virtual_sol_reserves.checked_mul(token_sell_proportion)?).checked_div(scaling_factor)?;
        
        // Ensure the amount to receive doesn't exceed the real SOL reserves
        let sol_to_receive = sol_to_receive.min(self.real_sol_reserves) ;

        // Update virtual and real reserves
        self.virtual_sol_reserves = self.virtual_sol_reserves.checked_sub(sol_to_receive)?;
        self.real_sol_reserves = self.real_sol_reserves.checked_sub(sol_to_receive)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buy_and_sell_too_much() {
        let mut amm = AMM::new();

        // Attempt to buy more tokens than available in reserves
        let buy_result = amm.apply_buy(25806975).unwrap();
        assert_eq!(buy_result.token_amount, 25806975);
        assert_eq!(buy_result.sol_amount, 54);
        assert_eq!(amm.real_token_reserves, 793074193024659);
        assert_eq!(amm.virtual_token_reserves, 1072974193024659000);
        assert_eq!(amm.real_sol_reserves, 1000054);
        assert_eq!(amm.virtual_sol_reserves, 30000000054);

        // Attempt to sell more tokens than available in reserves
        let sell_result = amm.apply_sell(793074193024659).unwrap();
        assert_eq!(sell_result.token_amount, 793074193024659);
        assert_eq!(sell_result.sol_amount, 1000054);
        assert_eq!(amm.real_sol_reserves, 0);
        assert_eq!(amm.virtual_sol_reserves, 29000000000);
        assert_eq!(amm.real_token_reserves, 1586148386049318);
        assert_eq!(amm.virtual_token_reserves, 1866048386049318000);
    }

    #[test]
    fn test_apply_sell() {
        let mut amm = AMM::new();
        let result = amm.apply_sell(25806975).unwrap();

        assert_eq!(result.token_amount, 25806975);
        assert_eq!(result.sol_amount, 53);
        assert_eq!(amm.virtual_token_reserves, 1073025806975000);
        assert_eq!(amm.real_token_reserves, 793125806975000);
        assert_eq!(amm.virtual_sol_reserves, 29999999947);
        assert_eq!(amm.real_sol_reserves, 999947);
    }

    #[test]
    fn test_get_sell_price() {
        let mut amm = AMM::new();

        // Edge case: zero tokens
        assert_eq!(amm.get_sell_price(0), None);

        // Normal case
        assert_eq!(amm.get_sell_price(25806975), Some(53));

        // Should not exceed real sol reserves
        assert_eq!(amm.get_sell_price(amm.virtual_token_reserves), Some(amm.real_sol_reserves));
    }

    #[test]
    fn test_apply_buy() {
        let mut amm = AMM::new();
        let result = amm.apply_buy(25806975).unwrap();

        assert_eq!(result.token_amount, 25806975);
        assert_eq!(result.sol_amount, 54);
        assert_eq!(amm.virtual_token_reserves, 1072974193025000);
        assert_eq!(amm.real_token_reserves, 793074193025000);
        assert_eq!(amm.virtual_sol_reserves, 30000000054);
        assert_eq!(amm.real_sol_reserves, 1000054);
    }

    #[test]
    fn test_get_buy_price() {
        let mut amm = AMM::new();

        // Edge case: zero SOL
        assert_eq!(amm.get_buy_price(0), None);

        // Normal case
        assert_eq!(amm.get_buy_price(54), Some(25806975));

        // Should not exceed real token reserves
        assert_eq!(amm.get_buy_price(amm.virtual_sol_reserves), Some(793099999999999));
    }
}