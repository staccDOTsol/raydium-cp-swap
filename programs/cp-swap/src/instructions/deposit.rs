use crate::curve::CurveCalculator;
use crate::curve::RoundDirection;
use crate::curve::AMM;
use crate::error::ErrorCode;
use crate::states::*;
use crate::utils::token::*;
use anchor_lang::prelude::*;
use anchor_spl::token::Token;
use anchor_spl::token_2022;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};

#[derive(Accounts)]
pub struct Deposit<'info> {
    /// Pays to mint the position
    pub owner: Signer<'info>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,

    /// Owner lp tokan account
    #[account(mut,  token::authority = owner)]
    pub owner_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The payer's token account for token_0
    #[account(
        mut,
        token::mint = token_0_vault.mint,
        token::authority = owner
    )]
    pub token_0_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The payer's token account for token_1
    #[account(
        mut,
        token::mint = token_1_vault.mint,
        token::authority = owner
    )]
    pub token_1_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that holds pool tokens for token_0
    #[account(
        mut,
        constraint = token_0_vault.key() == pool_state.load()?.token_0_vault
    )]
    pub token_0_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The address that holds pool tokens for token_1
    #[account(
        mut,
        constraint = token_1_vault.key() == pool_state.load()?.token_1_vault
    )]
    pub token_1_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// token Program
    pub token_program: Program<'info, Token2022>,

    /// Token program 2022
    pub token_program_2022: Program<'info, Token2022>,

    /// The mint of token_0 vault
    #[account(
        address = token_0_vault.mint
    )]
    pub vault_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of token_1 vault
    #[account(
        address = token_1_vault.mint
    )]
    pub vault_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Lp token mint
    #[account(
        mut,
        address = pool_state.load()?.lp_mint @ ErrorCode::IncorrectLpMint)
    ]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,
}
pub fn deposit(
    ctx: Context<Deposit>,
    token_0_amount: u64,
    token_1_amount: u64,
    minimum_lp_token_amount: u64,
) -> Result<()> {
    let pool_id = ctx.accounts.pool_state.key();
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit) {
        return err!(ErrorCode::NotApproved);
    }
    // Use AMM features to calculate the deposit amounts and LP tokens
    let token_0_amount = token_0_amount as u128;
    let token_1_amount = token_1_amount as u128;

    // Calculate the optimal deposit amounts
    let (optimal_token_0, optimal_token_1) = {
        let total_supply = pool_state.amm.virtual_token_0_reserves.checked_add(pool_state.amm.virtual_token_1_reserves)
            .ok_or(ErrorCode::IncorrectLpMint)?;
        let token_0_optimal = token_0_amount
            .checked_mul(pool_state.amm.virtual_token_1_reserves)
            .and_then(|v| v.checked_div(total_supply))
            .ok_or(ErrorCode::IncorrectLpMint)?;
        let token_1_optimal = token_1_amount
            .checked_mul(pool_state.amm.virtual_token_0_reserves)
            .and_then(|v| v.checked_div(total_supply))
            .ok_or(ErrorCode::IncorrectLpMint)?;
        (token_0_optimal, token_1_optimal)
    };

    // Determine the actual deposit amounts
    let (deposit_token_0, deposit_token_1) = if optimal_token_0 <= token_0_amount && optimal_token_1 <= token_1_amount {
        (optimal_token_0, optimal_token_1)
    } else if optimal_token_0.checked_mul(token_1_amount).unwrap() <= optimal_token_1.checked_mul(token_0_amount).unwrap() {
        (token_0_amount, optimal_token_1)
    } else {
        (optimal_token_0, token_1_amount)
    };

    // Calculate the LP tokens to mint
    let lp_token_amount = {
        let total_supply = pool_state.amm.virtual_token_0_reserves.checked_add(pool_state.amm.virtual_token_1_reserves)
            .ok_or(ErrorCode::IncorrectLpMint)?;
        deposit_token_0
            .checked_add(deposit_token_1)
            .and_then(|v| v.checked_mul(pool_state.lp_supply as u128))
            .and_then(|v| v.checked_div(total_supply))
            .ok_or(ErrorCode::IncorrectLpMint)?
    } as u64;

    // Ensure the calculated LP token amount meets the minimum requirement
    require!(
        lp_token_amount >= minimum_lp_token_amount,
        ErrorCode::IncorrectLpMint
    );

    // Transfer token_0 from user to pool
    transfer_from_user_to_pool_vault(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_0_account.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.vault_0_mint.to_account_info(),
        if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        },
        token_0_amount as u64,
        ctx.accounts.vault_0_mint.decimals,
    )?;

    // Transfer token_1 from user to pool
    transfer_from_user_to_pool_vault(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_1_account.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.vault_1_mint.to_account_info(),
        if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        },
        token_1_amount as u64,
        ctx.accounts.vault_1_mint.decimals,
    )?;
    let
     mut amm = pool_state.amm;
    amm.apply_deposit(deposit_token_0, deposit_token_1)?;
    pool_state.amm = amm;

    // Update pool state
    pool_state.lp_supply = pool_state.lp_supply.checked_add(lp_token_amount).unwrap();

    // Mint LP tokens to user
    token_mint_to(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
        ctx.accounts.lp_mint.to_account_info(),
        ctx.accounts.owner_lp_token.to_account_info(),
        lp_token_amount,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
    )?;

    pool_state.recent_epoch = Clock::get()?.epoch;

    Ok(())
}
