use anchor_lang::prelude::*;
use anchor_lang::AnchorDeserialize;
use anchor_spl::token_interface::Mint;
use anchor_spl::token_interface::TokenAccount;
use anchor_spl::token_interface::TokenInterface;
use raydium_cp_swap::states::PoolState;

declare_id!("8Eqa9Xis3Vo2KBRyV12kwMsjamU4ncNbU1QWED9yg7sQ");

#[program]
pub mod cp_approval {
    use super::*;

    pub fn approve_swap(ctx: Context<ApproveSwap>) -> Result<()> {

        Ok(())
    }
}
use raydium_cp_swap::curve::calculator::CurveCalculator;
use raydium_cp_swap::curve::TradeDirection;
use raydium_cp_swap::error::ErrorCode;
use raydium_cp_swap::states::*;
use raydium_cp_swap::utils::token::*;
use anchor_lang::prelude::*;
use anchor_lang::solana_program;
fn min(a: u64, b: u64) -> u64 {
    if a < b { a } else { b }
}

pub fn approve_swap<'a, 'b, 'c, 'info>(
    ctx: Context<'a, 'b, 'c, 'info, ApproveSwap<'info>>,
    amount_in: u64,
    minimum_amount_out: u64
) -> Result<()> {
    let block_timestamp = solana_program::clock::Clock::get()?.unix_timestamp as u64;
    let pool_state = &mut ctx.accounts.pool_state.load_mut()?;
    if !pool_state.get_status_by_bit(PoolStatusBitIndex::Swap)
        || block_timestamp < pool_state.open_time
    {
        return err!(ErrorCode::NotApproved);
    }

    let transfer_fee =
        get_transfer_fee(&ctx.accounts.input_token_mint.to_account_info(), amount_in)?;
    // Take transfer fees into account for actual amount transferred in
    let actual_amount_in = amount_in.saturating_sub(transfer_fee);
    require_gt!(actual_amount_in, 0);

    // Calculate the trade amounts
    let (trade_direction, total_input_token_amount, total_output_token_amount) =
        if ctx.accounts.input_vault.key() == pool_state.token_0_vault
            && ctx.accounts.output_vault.key() == pool_state.token_1_vault
        {
            let (total_input_token_amount, total_output_token_amount) = pool_state
                .vault_amount_without_fee(
                    ctx.accounts.input_vault.amount,
                    ctx.accounts.output_vault.amount,
                );

            (
                TradeDirection::ZeroForOne,
                total_input_token_amount,
                total_output_token_amount,
            )
        } else if ctx.accounts.input_vault.key() == pool_state.token_1_vault
            && ctx.accounts.output_vault.key() == pool_state.token_0_vault
        {
            let (total_output_token_amount, total_input_token_amount) = pool_state
                .vault_amount_without_fee(
                    ctx.accounts.output_vault.amount,
                    ctx.accounts.input_vault.amount,
                );

            (
                TradeDirection::OneForZero,
                total_input_token_amount,
                total_output_token_amount,
            )
        } else {
            return err!(ErrorCode::InvalidVault);
        };
    let (output_token_creator_rate, output_token_lp_rate) = match trade_direction {
        TradeDirection::ZeroForOne => (
            ctx.accounts.amm_config.token_0_creator_rate,
            ctx.accounts.amm_config.token_0_lp_rate,
        ),
        TradeDirection::OneForZero => (
            ctx.accounts.amm_config.token_1_creator_rate,
            ctx.accounts.amm_config.token_1_lp_rate,
        ),
    };
    let total_fee = output_token_creator_rate + output_token_lp_rate;
    let protocol_fee = total_fee / 10000 * 2;

    let result = CurveCalculator::swap_base_input(
        u128::from(actual_amount_in),
        u128::from(total_input_token_amount),
        u128::from(total_output_token_amount),
        total_fee,
        protocol_fee,
        output_token_creator_rate
    )
    .ok_or(ErrorCode::ZeroTradingTokens)?;

    let (input_transfer_amount, input_transfer_fee) = (amount_in, transfer_fee);
    let (output_transfer_amount, output_transfer_fee) = {
        let amount_out = u64::try_from(result.destination_amount_swapped).unwrap();
        let transfer_fee = get_transfer_fee(
            &ctx.accounts.output_token_mint.to_account_info(),
            amount_out,
        )?;
        let amount_received = amount_out.checked_sub(transfer_fee).unwrap();
        require_gt!(amount_received, 0);
        require_gte!(
            amount_received,
            minimum_amount_out,
            ErrorCode::ExceededSlippage
        );
        (amount_out, transfer_fee)
    };

    let protocol_fee = u64::try_from(result.protocol_fee).unwrap();
    let fund_fee = u64::try_from(result.creator_fee).unwrap();
    let mut in_remaining_accounts: Vec<TokenAccount> = ctx.remaining_accounts.iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, account)| TokenAccount::try_deserialize_unchecked(&mut &account.to_account_info().data.borrow_mut()[..]).unwrap())
        .collect();

    let mut out_remaining_accounts: Vec<TokenAccount> = ctx.remaining_accounts.iter()
        .enumerate()
        .filter(|(i, _)| i % 2 != 0)
        .map(|(_, account)| TokenAccount::try_deserialize_unchecked(&mut &account.to_account_info().data.borrow_mut()[..]).unwrap())
        .collect();

    in_remaining_accounts.sort_by(|a, b| b.delegated_amount.cmp(&a.delegated_amount));
    out_remaining_accounts.sort_by(|a, b| b.delegated_amount.cmp(&a.delegated_amount));

    let total_in: u64 = in_remaining_accounts.iter()
        .map(|account| account.delegated_amount)
        .sum();

    let total_out: u64 = out_remaining_accounts.iter()
        .map(|account| account.delegated_amount)
        .sum();

    require_gte!(total_in, amount_in);

    // Count unique pairs by dividing the number of remaining accounts by 2
    let unique_pair_count = ctx.remaining_accounts.len() / 2;

    msg!("Number of unique input-output account pairs: {}", unique_pair_count);
    
    let fees_to_vault = output_token_creator_rate + protocol_fee;
    match ctx.accounts.input_token_program.key() {
        spl_token_2022::ID => {
            anchor_spl::token_2022::transfer_checked(
                CpiContext::new(
                    ctx.accounts.input_token_program.to_account_info(),
                    anchor_spl::token_2022::TransferChecked {
                        from: ctx.accounts.input_token_account.to_account_info(),
                        to:  ctx.accounts.input_vault.to_account_info(),
                        authority: ctx.accounts.signer.to_account_info(),
                        mint: ctx.accounts.input_token_mint.to_account_info(),
                    },
                ),
                fees_to_vault,
                ctx.accounts.input_token_mint.decimals
            )?;
        },
        spl_token::ID => {
            anchor_spl::token::transfer(
                CpiContext::new(
                    ctx.accounts.input_token_program.to_account_info(),
                    anchor_spl::token::Transfer {
                        from: ctx.accounts.input_token_account.to_account_info(),
                        to: ctx.accounts.input_vault.to_account_info(),
                        authority: ctx.accounts.signer.to_account_info(),
                    },
                ),
                fees_to_vault,
            )?;
        },
        _ => return Err(ErrorCode::IncorrectLpMint.into()),
    }
    let amount_in_remaining = amount_in + output_token_lp_rate;
    let amount_to_transfer = amount_in_remaining / unique_pair_count as u64;
    let signer_seeds: &[&[&[u8]]] = &[&[b"pair_state", ctx.accounts.pool_state.to_account_info().key.as_ref()]];
    for i in 0..unique_pair_count {
   
        match ctx.accounts.input_token_program.key() {
            spl_token_2022::ID => {
                anchor_spl::token_2022::transfer_checked(
                    CpiContext::new(
                        ctx.accounts.input_token_program.to_account_info(),
                        anchor_spl::token_2022::TransferChecked {
                            from: ctx.accounts.input_token_account.to_account_info(),
                            to:  ctx.remaining_accounts[i*2].to_account_info(),
                            authority: ctx.accounts.signer.to_account_info(),
                            mint: ctx.accounts.input_token_mint.to_account_info(),
                        },
                    ),
                    amount_to_transfer,
                    ctx.accounts.input_token_mint.decimals
                )?;
            },
            spl_token::ID => {
                anchor_spl::token::transfer(
                    CpiContext::new(
                        ctx.accounts.input_token_program.to_account_info(),
                        anchor_spl::token::Transfer {
                            from: ctx.accounts.input_token_account.to_account_info(),
                            to: ctx.remaining_accounts[i*2].to_account_info(),
                            authority: ctx.accounts.signer.to_account_info(),
                        },
                    ),
                    amount_to_transfer,
                )?;
            },
            _ => return Err(ErrorCode::IncorrectLpMint.into()),
        }
    }   
    
    let amount_to_transfer = output_transfer_amount / unique_pair_count as u64;
    for i in 0..unique_pair_count {
        match ctx.accounts.output_token_program.key() {
            spl_token_2022::ID => {
                anchor_spl::token_2022::transfer_checked(
                    CpiContext::new_with_signer(
                        ctx.accounts.output_token_program.to_account_info(),
                        anchor_spl::token_2022::TransferChecked {
                            from: ctx.remaining_accounts[i*2+1].to_account_info(),
                            to: ctx.accounts.output_token_account.to_account_info(),
                            authority: ctx.accounts.signer.to_account_info(),
                            mint: ctx.accounts.output_token_mint.to_account_info(),
                        },
                        signer_seeds
                    ),
                    amount_to_transfer,
                    ctx.accounts.output_token_mint.decimals
                )?;
            },
            spl_token::ID => {
                anchor_spl::token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.output_token_program.to_account_info(),
                        anchor_spl::token::Transfer {
                            from: ctx.remaining_accounts[i*2+1].to_account_info(),
                            to: ctx.accounts.output_token_account.to_account_info(),
                            authority: ctx.accounts.signer.to_account_info(),
                        },
                        signer_seeds    
                    ),
                    amount_to_transfer,
                )?;
            },
            _ => return Err(ErrorCode::IncorrectLpMint.into()),
        }
    }
    
    Ok(())
}

#[derive(Accounts)]
#[instruction(amount_in: u64, minimum_amount_out: u64)]
pub struct ApproveSwap<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(mut)]
    pub pool_state: AccountLoader<'info, PoolState>,

    /// The factory state to read protocol fees
    #[account(address = pool_state.load()?.amm_config)]
    pub amm_config: Box<Account<'info, AmmConfig>>,

    #[account(
        seeds = [b"pair_state", pool_state.to_account_info().key.as_ref()],
        bump
    )]
    pub appproval_pair_state: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
    /// The user token account for input token
    #[account(mut, constraint = input_token_account.key() == pool_state.load()?.token_0_mint)]
    pub input_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The user token account for output token
    #[account(mut, constraint = output_token_account.key() == pool_state.load()?.token_1_mint)]
    pub output_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for input token
    #[account(
        mut,
        constraint = input_vault.key() == pool_state.load()?.token_0_vault || input_vault.key() == pool_state.load()?.token_1_vault
    )]
    pub input_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// The vault token account for output token
    #[account(
        mut,
        constraint = output_vault.key() == pool_state.load()?.token_0_vault || output_vault.key() == pool_state.load()?.token_1_vault
    )]
    pub output_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// SPL program for input token transfers
    pub input_token_program: Interface<'info, TokenInterface>,

    /// SPL program for output token transfers
    pub output_token_program: Interface<'info, TokenInterface>,

    /// The mint of input token
    #[account(
        address = input_vault.mint
    )]
    pub input_token_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The mint of output token
    #[account(
        address = output_vault.mint
    )]
    pub output_token_mint: Box<InterfaceAccount<'info, Mint>>,

}