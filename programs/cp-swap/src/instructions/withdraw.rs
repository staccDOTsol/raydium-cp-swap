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
pub struct Withdraw<'info> {
    /// Pays to burn the position
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

    /// Owner lp token account
    #[account(mut,  token::authority = owner)]
    pub owner_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The owner's token account for receive token_0
    #[account(
        mut,
        token::mint = token_0_vault.mint,
        token::authority = owner
    )]
    pub token_0_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The owner's token account for receive token_1
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

pub fn withdraw(
    ctx: Context<Withdraw>,
    lp_token_amount: u64,
    minimum_token_0_amount: u64,
    minimum_token_1_amount: u64,
) -> Result<()> {
    let pool_id = ctx.accounts.pool_state.key();
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    let pool_auth_bump = pool_state.auth_bump;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Withdraw) {
        return err!(ErrorCode::NotApproved);
    }

    // Calculate the withdrawal amounts
    let (withdraw_token_0, withdraw_token_1) = {
        let total_supply = pool_state.amm.virtual_token_0_reserves.checked_add(pool_state.amm.virtual_token_1_reserves)
            .ok_or(ErrorCode::IncorrectLpMint)?;
        let token_0_amount = (lp_token_amount as u128)
            .checked_mul(pool_state.amm.virtual_token_0_reserves)
            .and_then(|v| v.checked_div(total_supply))
            .ok_or(ErrorCode::IncorrectLpMint)?;
        let token_1_amount = (lp_token_amount as u128)
            .checked_mul(pool_state.amm.virtual_token_1_reserves)
            .and_then(|v| v.checked_div(total_supply))
            .ok_or(ErrorCode::IncorrectLpMint)?;
        (token_0_amount, token_1_amount)
    };

    // Ensure the calculated withdrawal amounts meet the minimum requirements
    require!(
        withdraw_token_0 >= minimum_token_0_amount as u128 && withdraw_token_1 >= minimum_token_1_amount as u128,
        ErrorCode::ExceededSlippage
    );

    // Burn LP tokens from user
    token_burn(
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
        ctx.accounts.lp_mint.to_account_info(),
        ctx.accounts.owner_lp_token.to_account_info(),
        lp_token_amount,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_auth_bump]]],
    )?;

    // Transfer token_0 from pool to user
    transfer_from_pool_vault_to_user(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.token_0_account.to_account_info(),
        ctx.accounts.vault_0_mint.to_account_info(),
        if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        },
        withdraw_token_0 as u64,
        ctx.accounts.vault_0_mint.decimals,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_auth_bump]]],
    )?;

    // Transfer token_1 from pool to user
    transfer_from_pool_vault_to_user(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_account.to_account_info(),
        ctx.accounts.vault_1_mint.to_account_info(),
        if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
            ctx.accounts.token_program.to_account_info()
        } else {
            ctx.accounts.token_program_2022.to_account_info()
        },
        withdraw_token_1 as u64,
        ctx.accounts.vault_1_mint.decimals,
        &[&[crate::AUTH_SEED.as_bytes(), &[pool_auth_bump]]],
    )?;
    let mut amm = pool_state.amm;
    amm.apply_withdraw(withdraw_token_0, withdraw_token_1)?;
    pool_state.amm = amm;
    // Update pool state
    pool_state.lp_supply = pool_state.lp_supply.checked_sub(lp_token_amount).unwrap();

    pool_state.recent_epoch = Clock::get()?.epoch;

    Ok(())
}
