use crate::curve::CurveCalculator;
use crate::curve::AMM;
use crate::error::ErrorCode;
use crate::states::*;
use crate::utils::*;

use anchor_lang::{
    accounts::interface_account::InterfaceAccount, prelude::*, solana_program::clock,
    system_program,
};
use anchor_spl::metadata::Metadata;

use anchor_spl::{
    associated_token::AssociatedToken,
    token::Token,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use std::ops::Deref;
#[derive(Accounts)]
#[instruction(
    init_amount_0: u64,
    init_amount_1: u64,
    open_time: u64,
)]
pub struct Initialize<'info> {
    #[account(
        mut,
        constraint = creator.key() == amm_config.fund_owner 
    )]
    pub creator: Signer<'info>,
    /// Which config the pool belongs to.
    pub amm_config: Box<Account<'info, AmmConfig>>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: Initialize an account to store the pool state
    /// PDA account:
    /// seeds = [
    ///     POOL_SEED.as_bytes(),
    ///     amm_config.key().as_ref(),
    ///     token_0_mint.key().as_ref(),
    ///     token_1_mint.key().as_ref(),
    /// ],
    ///
    /// Or random account: must be signed by cli
    #[account(mut)]
    pub pool_state: UncheckedAccount<'info>,

    /// Token_0 mint, the key must smaller then token_1 mint.
    #[account(
        constraint = token_0_mint.key() < token_1_mint.key(),
        mint::token_program = token_0_program,
    )]
    pub token_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Token_1 mint, the key must grater then token_0 mint.
    #[account(
        mint::token_program = token_1_program,
    )]
    pub token_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// pool lp mint
    #[account(
        init,
        seeds = [
            "pool_lp_mint".as_bytes(),
            pool_state.key().as_ref(),
        ],
        bump,
        mint::decimals = 9,
        mint::authority = authority,
        payer = creator,
        mint::token_program = token_program,
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    /// payer token0 account
    #[account(
        mut,
        token::mint = token_0_mint,
        token::authority = creator,
    )]
    pub creator_token_0: Box<InterfaceAccount<'info, TokenAccount>>,

    /// creator token1 account
    #[account(
        mut,
        token::mint = token_1_mint,
        token::authority = creator,
    )]
    pub creator_token_1: Box<InterfaceAccount<'info, TokenAccount>>,
    /// creator lp token account
    /// creator lp token account
    #[account(
        init,
        associated_token::mint = lp_mint,
        associated_token::authority = creator,
        payer = creator,
        token::token_program = token_program,
    )]
    pub creator_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Token_0 vault for the pool
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_0_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_0_vault: UncheckedAccount<'info>,

    /// CHECK: Token_1 vault for the pool
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_1_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_1_vault: UncheckedAccount<'info>,

    /// Program to create mint account and mint tokens
    pub token_program: Program<'info, Token>,
    /// Spl token program or token program 2022
    pub token_0_program: Interface<'info, TokenInterface>,
    /// Spl token program or token program 2022
    pub token_1_program: Interface<'info, TokenInterface>,
    /// Program to create an ATA for receiving position NFT
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// To create a new program account
    pub system_program: Program<'info, System>,
    /// Sysvar for program account
    pub rent: Sysvar<'info, Rent>,
    /// CHECK: This is not dangerous because we don't read or write from this account
    pub observation_state: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct InitializeMetadata<'info> {
    #[account(
        mut,
        constraint = creator.key() == amm_config.fund_owner 
    )]
    pub creator: Signer<'info>,
    /// CHECK: This is not dangerous because we don't read or write from this account
    /// CHECK: pool vault and lp mint authority
    #[account(
    seeds = [
        crate::AUTH_SEED.as_bytes(),
    ],
    bump,
)]
    pub authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_metadata_program: Program<'info, Metadata>,
    #[account(
        mut,
        seeds = [
            b"metadata", 
            token_metadata_program.key.as_ref(), 
            lp_mint.to_account_info().key.as_ref()
        ],
        seeds::program = token_metadata_program.key(),
        bump,
    )]
    pub metadata: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    /// AMM config account, used to verify the creator
    pub amm_config: Account<'info, AmmConfig>,

    /// an account to store oracle observations
    #[account(
        init,
        seeds = [
            OBSERVATION_SEED.as_bytes(),
            pool_state.key().as_ref(),
        ],
        bump,
        payer = creator,
        space = ObservationState::LEN
    )]
    pub observation_state: AccountLoader<'info, ObservationState>,
    /// The pool state account
    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,
}

pub fn initialize_metadata(
    ctx: Context<InitializeMetadata>,
) -> Result<()> {
  
    let mut observation_state = ctx.accounts.observation_state.load_init()?;
    observation_state.pool_id = ctx.accounts.pool_state.key();

    Ok(())
}

pub fn initialize(
    ctx: Context<Initialize>,
    init_amount_0: u64,
    init_amount_1: u64,
    mut open_time: u64,
) -> Result<()> {
    //  if !(is_supported_mint(&ctx.accounts.token_0_mint).unwrap()
    //     && is_supported_mint(&ctx.accounts.token_1_mint).unwrap())
    //{
    //   return err!(ErrorCode::NotSupportMint);
    //   }

    if ctx.accounts.amm_config.disable_create_pool {
        return err!(ErrorCode::NotApproved);
    }
    let block_timestamp = clock::Clock::get()?.unix_timestamp as u64;
    if open_time <= block_timestamp {
        open_time = block_timestamp + 1;
    }
    // due to stack/heap limitations, we have to create redundant new accounts ourselves.
    create_token_account(
        &ctx.accounts.authority.to_account_info(),
        &ctx.accounts.creator.to_account_info(),
        &ctx.accounts.token_0_vault.to_account_info(),
        &ctx.accounts.token_0_mint.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.token_0_program.to_account_info(),
        &[&[
            POOL_VAULT_SEED.as_bytes(),
            ctx.accounts.pool_state.key().as_ref(),
            ctx.accounts.token_0_mint.key().as_ref(),
            &[ctx.bumps.token_0_vault][..],
        ][..]],
    )?;

    create_token_account(
        &ctx.accounts.authority.to_account_info(),
        &ctx.accounts.creator.to_account_info(),
        &ctx.accounts.token_1_vault.to_account_info(),
        &ctx.accounts.token_1_mint.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.token_1_program.to_account_info(),
        &[&[
            POOL_VAULT_SEED.as_bytes(),
            ctx.accounts.pool_state.key().as_ref(),
            ctx.accounts.token_1_mint.key().as_ref(),
            &[ctx.bumps.token_1_vault][..],
        ][..]],
    )?;

    let pool_state_loader = create_pool(
        &ctx.accounts.creator.to_account_info(),
        &ctx.accounts.pool_state.to_account_info(),
        &ctx.accounts.amm_config.to_account_info(),
        &ctx.accounts.token_0_mint.to_account_info(),
        &ctx.accounts.token_1_mint.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
    )?;
    let pool_state = &mut pool_state_loader.load_init()?;

    let liquidity = U128::from(init_amount_0)
        .checked_mul(init_amount_1.into())
        .unwrap()
        .integer_sqrt()
        .as_u64();
    let mut amm = AMM::new();
    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_0.to_account_info(),
        ctx.accounts.token_0_vault.to_account_info(),
        ctx.accounts.token_0_mint.to_account_info(),
        ctx.accounts.token_0_program.to_account_info(),
        amm.get_buy_price(liquidity.into()).unwrap() as u64 * init_amount_0,
        ctx.accounts.token_0_mint.decimals,
    )?;

    transfer_from_user_to_pool_vault(
        ctx.accounts.creator.to_account_info(),
        ctx.accounts.creator_token_1.to_account_info(),
        ctx.accounts.token_1_vault.to_account_info(),
        ctx.accounts.token_1_mint.to_account_info(),
        ctx.accounts.token_1_program.to_account_info(),
        amm.apply_buy(liquidity.into()).unwrap().sol_amount as u64 * init_amount_1,
        ctx.accounts.token_1_mint.decimals,
    )?;

    pool_state.amm = amm;
    let token_0_vault =
        spl_token_2022::extension::StateWithExtensions::<spl_token_2022::state::Account>::unpack(
            ctx.accounts
                .token_0_vault
                .to_account_info()
                .try_borrow_data()?
                .deref(),
        )?
        .base;
    let token_1_vault =
        spl_token_2022::extension::StateWithExtensions::<spl_token_2022::state::Account>::unpack(
            ctx.accounts
                .token_1_vault
                .to_account_info()
                .try_borrow_data()?
                .deref(),
        )?
        .base;

    CurveCalculator::validate_supply(token_0_vault.amount, token_1_vault.amount)?;

    let lock_lp_amount = 100;
    msg!(
        "liquidity:{}, lock_lp_amount:{}, vault_0_amount:{},vault_1_amount:{}",
        liquidity,
        lock_lp_amount,
        token_0_vault.amount,
        token_1_vault.amount
    );
    // Mint LP tokens

    pool_state.lp_supply = pool_state
        .lp_supply
        .checked_add(liquidity.try_into().unwrap())
        .unwrap();

    crate::utils::token_mint_to(
        ctx.accounts.authority.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
        ctx.accounts.lp_mint.to_account_info(),
        ctx.accounts.creator_lp_token.to_account_info(),
        (liquidity as u64)
            .checked_sub(lock_lp_amount)
            .ok_or(ErrorCode::InitLpAmountTooLess)?,
        &[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]],
    )?;

    pool_state.initialize(
        ctx.bumps.authority,
        liquidity as u64,
        open_time,
        ctx.accounts.creator.key(),
        ctx.accounts.amm_config.key(),
        ctx.accounts.token_0_vault.key(),
        ctx.accounts.token_1_vault.key(),
        &ctx.accounts.token_0_mint,
        &ctx.accounts.token_1_mint,
        &ctx.accounts.lp_mint.key(),
        ctx.accounts.observation_state.key(),
    );

    Ok(())
}

pub fn create_pool<'info>(
    payer: &AccountInfo<'info>,
    pool_account_info: &AccountInfo<'info>,
    amm_config: &AccountInfo<'info>,
    token_0_mint: &AccountInfo<'info>,
    token_1_mint: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
) -> Result<AccountLoad<'info, PoolState>> {
    if pool_account_info.owner != &system_program::ID || pool_account_info.lamports() != 0 {
        return err!(ErrorCode::NotApproved);
    }

    let (expect_pda_address, bump) = Pubkey::find_program_address(
        &[
            POOL_SEED.as_bytes(),
            amm_config.key().as_ref(),
            token_0_mint.key().as_ref(),
            token_1_mint.key().as_ref(),
        ],
        &crate::id(),
    );

    if pool_account_info.key() != expect_pda_address {
        require_eq!(pool_account_info.is_signer, true);
    }

    let cpi_accounts = anchor_lang::system_program::CreateAccount {
        from: payer.clone(),
        to: pool_account_info.clone(),
    };
    let cpi_context = CpiContext::new(system_program.to_account_info(), cpi_accounts);
    anchor_lang::system_program::create_account(
        cpi_context.with_signer(&[&[
            POOL_SEED.as_bytes(),
            amm_config.key().as_ref(),
            token_0_mint.key().as_ref(),
            token_1_mint.key().as_ref(),
            &[bump],
        ][..]]),
        Rent::get()?.minimum_balance(PoolState::LEN),
        PoolState::LEN as u64,
        &crate::id(),
    )?;

    Ok(AccountLoad::<PoolState>::try_from_unchecked(
        &crate::id(),
        &pool_account_info,
    )?)
}
