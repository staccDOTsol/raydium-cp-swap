use crate::curve::CurveCalculator;
use crate::curve::RoundDirection;
use crate::error::ErrorCode;
use crate::states::*;
use crate::utils::token::*;
use crate::utils::U128;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::spl_associated_token_account::instruction::create_associated_token_account;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::metadata::create_metadata_accounts_v3;
use anchor_spl::metadata::CreateMetadataAccountsV3;
use anchor_spl::metadata::MetadataAccount;
use anchor_spl::token::Token;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
use mpl_token_metadata::types::DataV2;
use spl_memo::solana_program::program::invoke_signed;

#[derive(Accounts)]
pub struct Deposit<'info> {
    /// Pays to mint the position
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        mut,
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
    pub token_program: Program<'info, Token>,

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
    pub spl_memo: UncheckedAccount<'info>,
    /// pool lp mint
    #[account(
    init_if_needed,
    seeds = [
        "pool_lp_mint_safu".as_bytes(),
        pool_state.key().as_ref(),
    ],
    bump,
    mint::decimals = 9,
    mint::authority = authority,
    payer = owner,
    mint::token_program = token_program,
)]
    pub lp_mint_safu: Box<InterfaceAccount<'info, Mint>>,
    pub system_program: Program<'info, System>,

    /// CHECK: Metadata account for the original LP mint
    pub metadata_account: UncheckedAccount<'info>,

    /// CHECK: New metadata account for the SAFU LP mint
    #[account(mut)]
    pub new_metadata_account: UncheckedAccount<'info>,

    pub rent: UncheckedAccount<'info>,

    /// CHECK: Metaplex token metadata program
    pub metadata_program: UncheckedAccount<'info>,
    pub associated_token_program: Program<'info, AssociatedToken>,

    /// CHECK: Token_1 vault for the pool
    #[account(
        mut,
        seeds = [
            "pool_vault_safu".as_bytes(),
            pool_state.key().as_ref(),
            vault_1_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_1_vault_safu: UncheckedAccount<'info>,

    /// CHECK: Token_1 vault for the pool
    #[account(
        mut,
        seeds = [
            "pool_vault_safu".as_bytes(),
            pool_state.key().as_ref(),
            vault_0_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_0_vault_safu: UncheckedAccount<'info>,
}

pub fn deposit<'info>(
    ctx: Context<'_, '_, '_, 'info, Deposit<'info>>,
    lp_token_amount: u64,
    maximum_token_0_amount: u64,
    maximum_token_1_amount: u64,
) -> Result<()> {
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    if pool_state.token_0_vault_safu == Pubkey::default()
        || pool_state.token_1_vault_safu == Pubkey::default()
    {
        pool_state.token_0_vault_safu = ctx.accounts.token_0_vault_safu.key();
        pool_state.token_1_vault_safu = ctx.accounts.token_1_vault_safu.key();
    }
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Deposit) {
        return err!(ErrorCode::NotApproved);
    }
    let new_metadata_account = &ctx.accounts.new_metadata_account;
    let metadata_account = &ctx.accounts.metadata_account;
    let metadata_program = &ctx.accounts.metadata_program;
    let lp_mint_safu = &ctx.accounts.lp_mint_safu;
    let system_program = &ctx.accounts.system_program;
    let remaining_accounts = &ctx.remaining_accounts;

    let mut iter = remaining_accounts.iter();
    // Skip the first two accounts (owner_lp_token_safu and token_0_vault_safu)
    let owner_lp_token_safu = next_account_info(&mut iter)?;
    let token_0_mint = next_account_info(&mut iter)?;
    let token_1_mint = next_account_info(&mut iter)?;
    let token_0_vault_safu = &mut ctx.accounts.token_0_vault_safu;
    let token_1_vault_safu = &mut ctx.accounts.token_1_vault_safu;

    // Get metadata from current LP mint
    let token_program_2022 = &ctx.accounts.token_program_2022;
    let token_program = &ctx.accounts.token_program;

    // Create owner's SAFU LP token account if it doesn't exist
    if owner_lp_token_safu.data_is_empty() {
        let create_ata_ix = create_associated_token_account(
            &ctx.accounts.owner.key(),
            &ctx.accounts.owner.key(),
            &lp_mint_safu.key(),
            if lp_mint_safu.to_account_info().owner == &ctx.accounts.token_program.key() {
                &token_program.to_account_info().key
            } else {
                &token_program_2022.to_account_info().key
            },
        );
        invoke_signed(
            &create_ata_ix,
            &[
                ctx.accounts.authority.to_account_info(),
                ctx.accounts.owner.to_account_info(),
                owner_lp_token_safu.to_account_info(),
                lp_mint_safu.to_account_info(),
                system_program.to_account_info(),
                if lp_mint_safu.to_account_info().owner == &ctx.accounts.token_program.key() {
                    ctx.accounts.token_program.to_account_info()
                } else {
                    ctx.accounts.token_program_2022.to_account_info()
                },
            ],
            &[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]],
        )?;
    }
    // Create SAFU LP mint if it doesn't exist
    if new_metadata_account.data_is_empty() {
        // Create account for SAFU LP mint

        // Create token accounts for SAFU vaults if they don't exist
        if token_0_vault_safu.data_is_empty() {
            // Derive the PDA for the token_0_vault_safu account
            let (token_0_vault_safu_pda, bump_0) = Pubkey::find_program_address(
                &[
                    b"pool_vault_safu",
                    ctx.accounts.pool_state.key().as_ref(),
                    ctx.accounts.vault_0_mint.key().as_ref(),
                ],
                ctx.program_id,
            );

            // Ensure the derived PDA matches the provided account
            if token_0_vault_safu.key() != token_0_vault_safu_pda {
                return Err(ErrorCode::InvalidVault.into());
            }

            // Create the token account for SAFU vault 0
            create_token_account(
                &ctx.accounts.authority.to_account_info(),
                &ctx.accounts.owner.to_account_info(),
                &token_0_vault_safu.to_account_info(),
                &token_0_mint.to_account_info(),
                &system_program.to_account_info(),
                if ctx.accounts.vault_0_mint.to_account_info().owner
                    == &ctx.accounts.token_program.key()
                {
                    &token_program
                } else {
                    &token_program_2022
                },
                &[&[
                    "pool_vault_safu".as_bytes(),
                    ctx.accounts.pool_state.key().as_ref(),
                    ctx.accounts.vault_0_mint.key().as_ref(),
                    &[bump_0][..],
                ][..]],
            )?;
        }

        if token_1_vault_safu.data_is_empty() {
            // Derive the PDA for the token_1_vault_safu account
            let (token_1_vault_safu_pda, bump_1) = Pubkey::find_program_address(
                &[
                    b"pool_vault_safu",
                    ctx.accounts.pool_state.key().as_ref(),
                    ctx.accounts.vault_1_mint.key().as_ref(),
                ],
                ctx.program_id,
            );

            // Ensure the derived PDA matches the provided account
            if token_1_vault_safu.key() != token_1_vault_safu_pda {
                return Err(ErrorCode::InvalidVault.into());
            }

            // Create the token account for SAFU vault 1
            create_token_account(
                &ctx.accounts.authority.to_account_info(),
                &ctx.accounts.owner.to_account_info(),
                &token_1_vault_safu.to_account_info(),
                &token_1_mint.to_account_info(),
                &system_program.to_account_info(),
                if ctx.accounts.vault_1_mint.to_account_info().owner
                    == &ctx.accounts.token_program.key()
                {
                    &token_program
                } else {
                    &token_program_2022
                },
                &[&[
                    "pool_vault_safu".as_bytes(),
                    ctx.accounts.pool_state.key().as_ref(),
                    ctx.accounts.vault_1_mint.key().as_ref(),
                    &[bump_1][..],
                ][..]],
            )?;
        }

        // Derive the metadata account address for the SAFU LP mint
        let metadata_seeds = &[
            b"metadata".as_ref(),
            mpl_token_metadata::ID.as_ref(),
            lp_mint_safu.to_account_info().key.as_ref(),
        ];
        let (metadata_pda, _) =
            Pubkey::find_program_address(metadata_seeds, &mpl_token_metadata::ID);

        // Ensure the provided new_metadata_account matches the derived PDA
        if metadata_pda != *new_metadata_account.key {
            return Err(ErrorCode::InvalidInput.into());
        }

        // Read existing metadata
        let existing_metadata = MetadataAccount::try_deserialize_unchecked(
            &mut &metadata_account.data.borrow_mut()[..],
        )?;

        // Create metadata for SAFU LP mint
        create_metadata_accounts_v3(
            CpiContext::new_with_signer(
                metadata_program.to_account_info(),
                CreateMetadataAccountsV3 {
                    metadata: new_metadata_account.to_account_info(),
                    mint: lp_mint_safu.to_account_info(),
                    mint_authority: ctx.accounts.authority.to_account_info(),
                    payer: ctx.accounts.owner.to_account_info(),
                    update_authority: ctx.accounts.authority.to_account_info(),
                    system_program: system_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                },
                &[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]],
            ),
            DataV2 {
                name: existing_metadata.name.clone(),
                symbol: existing_metadata.symbol.clone(),
                uri: existing_metadata.uri.clone(),
                seller_fee_basis_points: 0,
                creators: None,
                collection: None,
                uses: None,
            },
            true,
            true,
            None,
        )?;
        let liquidity = U128::from(100000)
            .checked_mul(100000.into())
            .unwrap()
            .integer_sqrt()
            .as_u64();
        transfer_from_user_to_pool_vault(
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_0_account.to_account_info(),
            token_0_vault_safu.to_account_info(),
            ctx.accounts.vault_0_mint.to_account_info(),
            if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
            100000,
            ctx.accounts.vault_0_mint.decimals,
        )?;

        transfer_from_user_to_pool_vault(
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_1_account.to_account_info(),
            token_1_vault_safu.to_account_info(),
            ctx.accounts.vault_1_mint.to_account_info(),
            if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
            100000,
            ctx.accounts.vault_1_mint.decimals,
        )?;

        let token_0_vault = spl_token_2022::extension::StateWithExtensions::<
            spl_token_2022::state::Account,
        >::unpack(
            *token_0_vault_safu.to_account_info().try_borrow_data()?
        )?
        .base;
        let token_1_vault = spl_token_2022::extension::StateWithExtensions::<
            spl_token_2022::state::Account,
        >::unpack(
            *token_1_vault_safu.to_account_info().try_borrow_data()?
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

        pool_state.safu_lp_supply = pool_state
            .safu_lp_supply
            .checked_add(liquidity.try_into().unwrap())
            .unwrap();
    }
    let is_safu = ctx.remaining_accounts.len() > 1;
    if is_safu {
        // Calculate LP tokens for SAFU (99.9%)
        let safu_lp_token_amount = lp_token_amount
            .checked_mul(999)
            .and_then(|amount: u64| amount.checked_div(1000))
            .ok_or(ErrorCode::EmptySupply)?;
        let token_0_vault_safu_amount = spl_token_2022::extension::StateWithExtensions::<
            spl_token_2022::state::Account,
        >::unpack(
            *token_0_vault_safu.to_account_info().try_borrow_data()?
        )?
        .base
        .amount;
        let token_1_vault_safu_amount = spl_token_2022::extension::StateWithExtensions::<
            spl_token_2022::state::Account,
        >::unpack(
            *token_1_vault_safu.to_account_info().try_borrow_data()?
        )?
        .base
        .amount;
        let (total_token_0_amount, total_token_1_amount) = pool_state
            .vault_amount_without_fee(token_0_vault_safu_amount, token_1_vault_safu_amount);

        // Calculate token amounts for SAFU
        let safu_results = CurveCalculator::lp_tokens_to_trading_tokens(
            u128::from(safu_lp_token_amount),
            u128::from(pool_state.safu_lp_supply),
            u128::from(total_token_0_amount),
            u128::from(total_token_1_amount),
            RoundDirection::Ceiling,
        )
        .ok_or(ErrorCode::ZeroTradingTokens)?;

        let safu_token_0_amount = u64::try_from(safu_results.token_0_amount).unwrap();
        let (safu_transfer_token_0_amount, _safu_transfer_token_0_fee) = {
            let transfer_fee = get_transfer_inverse_fee(
                &ctx.accounts.vault_0_mint.to_account_info(),
                safu_token_0_amount,
            )?;
            (
                safu_token_0_amount.checked_add(transfer_fee).unwrap(),
                transfer_fee,
            )
        };

        let safu_token_1_amount = u64::try_from(safu_results.token_1_amount).unwrap();
        let (safu_transfer_token_1_amount, _safu_transfer_token_1_fee) = {
            let transfer_fee = get_transfer_inverse_fee(
                &ctx.accounts.vault_1_mint.to_account_info(),
                safu_token_1_amount,
            )?;
            (
                safu_token_1_amount.checked_add(transfer_fee).unwrap(),
                transfer_fee,
            )
        };

        #[cfg(feature = "enable-log")]
        msg!(
        "safu_results.token_0_amount:{}, safu_results.token_1_amount:{},safu_transfer_token_0_amount:{},safu_transfer_token_0_fee:{},
            safu_transfer_token_1_amount:{},safu_transfer_token_1_fee:{}",
        safu_results.token_0_amount,
        safu_results.token_1_amount,
        safu_transfer_token_0_amount,
        safu_transfer_token_0_fee,
        safu_transfer_token_1_amount,
        safu_transfer_token_1_fee
    );

        // Transfer tokens to SAFU vaults
        transfer_from_user_to_pool_vault(
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_0_account.to_account_info(),
            token_0_vault_safu.to_account_info(),
            ctx.accounts.vault_0_mint.to_account_info(),
            if ctx.accounts.vault_0_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
            safu_transfer_token_0_amount,
            ctx.accounts.vault_0_mint.decimals,
        )?;

        transfer_from_user_to_pool_vault(
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_1_account.to_account_info(),
            token_1_vault_safu.to_account_info(),
            ctx.accounts.vault_1_mint.to_account_info(),
            if ctx.accounts.vault_1_mint.to_account_info().owner == ctx.accounts.token_program.key {
                ctx.accounts.token_program.to_account_info()
            } else {
                ctx.accounts.token_program_2022.to_account_info()
            },
            safu_transfer_token_1_amount,
            ctx.accounts.vault_1_mint.decimals,
        )?;

        // Mint SAFU LP tokens
        token_mint_to(
            ctx.accounts.authority.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            lp_mint_safu.to_account_info(),
            owner_lp_token_safu.to_account_info(),
            safu_lp_token_amount,
            &[&[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]]],
        )?;

        // Update pool state for SAFU
        pool_state.safu_lp_supply = pool_state
            .safu_lp_supply
            .checked_add(safu_lp_token_amount.try_into().unwrap())
            .unwrap();
        pool_state.recent_epoch = Clock::get()?.epoch;
    } else {
        let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
            ctx.accounts.token_0_vault.amount,
            ctx.accounts.token_1_vault.amount,
        );
        let results = CurveCalculator::lp_tokens_to_trading_tokens(
            u128::from(lp_token_amount),
            u128::from(pool_state.lp_supply),
            u128::from(total_token_0_amount),
            u128::from(total_token_1_amount),
            RoundDirection::Ceiling,
        )
        .ok_or(ErrorCode::ZeroTradingTokens)?;

        let token_0_amount = u64::try_from(results.token_0_amount).unwrap();
        let (transfer_token_0_amount, _transfer_token_0_fee) = {
            let transfer_fee = get_transfer_inverse_fee(
                &ctx.accounts.vault_0_mint.to_account_info(),
                token_0_amount,
            )?;
            (
                token_0_amount.checked_add(transfer_fee).unwrap(),
                transfer_fee,
            )
        };

        let token_1_amount = u64::try_from(results.token_1_amount).unwrap();
        let (transfer_token_1_amount, _transfer_token_1_fee) = {
            let transfer_fee = get_transfer_inverse_fee(
                &ctx.accounts.vault_1_mint.to_account_info(),
                token_1_amount,
            )?;
            (
                token_1_amount.checked_add(transfer_fee).unwrap(),
                transfer_fee,
            )
        };

        #[cfg(feature = "enable-log")]
        msg!(
        "results.token_0_amount;{}, results.token_1_amount:{},transfer_token_0_amount:{},transfer_token_0_fee:{},
            transfer_token_1_amount:{},transfer_token_1_fee:{}",
        results.token_0_amount,
        results.token_1_amount,
        transfer_token_0_amount,
        transfer_token_0_fee,
        transfer_token_1_amount,
        transfer_token_1_fee
    );

        if transfer_token_0_amount > maximum_token_0_amount
            || transfer_token_1_amount > maximum_token_1_amount
        {
            return Err(ErrorCode::ExceededSlippage.into());
        }

        let mut amm = pool_state.amm;

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
            amm.get_buy_price(lp_token_amount.into()).unwrap() as u64 * transfer_token_0_amount,
            ctx.accounts.vault_0_mint.decimals,
        )?;

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
            amm.apply_buy(lp_token_amount.into()).unwrap().sol_amount as u64
                * transfer_token_1_amount,
            ctx.accounts.vault_1_mint.decimals,
        )?;

        // Calculate LP tokens to mint based on deposit amount
        pool_state.amm = amm;
        pool_state.lp_supply = pool_state
            .lp_supply
            .checked_add(lp_token_amount.try_into().unwrap())
            .unwrap();

        token_mint_to(
            ctx.accounts.authority.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.lp_mint.to_account_info(),
            ctx.accounts.owner_lp_token.to_account_info(),
            lp_token_amount as u64,
            &[&[crate::AUTH_SEED.as_bytes(), &[pool_state.auth_bump]]],
        )?;
        pool_state.recent_epoch = Clock::get()?.epoch;
    }
    Ok(())
}
