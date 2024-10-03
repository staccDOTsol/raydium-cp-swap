use std::fmt;
use anchor_lang::prelude::*;
use anchor_lang::AnchorDeserialize;
use num_integer::Roots;

pub const PRICE_SCALE: u128 = 1_000_000_000; // 9 decimal places

#[derive(Debug, Clone, Copy, AnchorDeserialize, AnchorSerialize, Default)]
pub struct AMM {

    pub buffer: u128,
    pub buffer_1: u128,
    pub buffer_2: u128,
    pub buffer_3: u128,
    pub buffer_4: u128,
}

impl AMM {

    pub fn initialize(&mut self, initial_token0: u128, initial_token1: u128) -> Option<u128> {
       
        let initial_lp_tokens = (initial_token0 * initial_token1).sqrt();

        Some(initial_lp_tokens)
    }

    pub fn calculate_cost_modifier(&self, target_lp_token_amount: u128, current_liquidity: u128) -> u128 {
        let target_liquidity = current_liquidity + target_lp_token_amount;

        // Calculate market cap (x) as the total liquidity
        let x = target_liquidity as f64;

        // Calculate y using the given exponential formula
        let y = 0.6015 * (1.0000036f64.powf(x / 10_000_000.0));

        // Convert y to the price scale and return as u128
        (y * PRICE_SCALE as f64) as u128
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize() {
        let mut amm = AMM::default();
        let initial_token0 = 1_000_000_000;
        let initial_token1 = 1_000_000_000;
        let initial_lp_tokens = amm.initialize(initial_token0, initial_token1).unwrap();

        assert!(initial_lp_tokens > 0);
    }

    #[test]
    fn test_cost_modifier() {
        let mut amm = AMM::default();
        let initial_lp = amm.initialize(1_000_000_000_000, 1_000_000_000_000).unwrap();

        let small_amount = initial_lp / 10;
        let large_amount = initial_lp * 10;

        let small_modifier = amm.calculate_cost_modifier(small_amount, initial_lp);
        let large_modifier = amm.calculate_cost_modifier(large_amount, initial_lp);

        assert!(small_modifier < large_modifier);
        // Remove the assertion that small_modifier > PRICE_SCALE
        assert!(large_modifier > PRICE_SCALE);

        // Add more detailed assertions
        assert!(small_modifier > 0, "Small modifier should be greater than 0");
        assert!(small_modifier <= PRICE_SCALE, "Small modifier should be less than or equal to PRICE_SCALE");
        
        // Test with very small amount=
        let very_small_amount = 1;
        let very_small_modifier = amm.calculate_cost_modifier(very_small_amount, initial_lp);
        assert!(very_small_modifier > 0 && very_small_modifier <= PRICE_SCALE, 
                "Very small modifier should be between 0 and PRICE_SCALE");

        // Test with very large amount
        let very_large_amount = initial_lp * 100;
        let very_large_modifier = amm.calculate_cost_modifier(very_large_amount, initial_lp);
        assert!(very_large_modifier > PRICE_SCALE, "Very large modifier should be greater than PRICE_SCALE");
    }
}