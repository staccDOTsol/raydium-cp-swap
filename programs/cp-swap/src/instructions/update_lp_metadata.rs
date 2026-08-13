use crate::error::ErrorCode;
use crate::states::*;
use anchor_lang::prelude::*;
use anchor_spl::metadata::mpl_token_metadata::types::DataV2;
use anchor_spl::metadata::{self, Metadata};
use anchor_spl::token_interface::Mint;

#[derive(Accounts)]
pub struct UpdateLpMetadata<'info> {
    /// The creator who first created the LP metadata, the only one allowed to update it.
    pub creator: Signer<'info>,

    /// Pool state, used to derive the lp mint and the metadata state PDA.
    pub pool_state: AccountLoader<'info, PoolState>,

    /// The LP mint whose metadata is being updated.
    #[account(
        address = pool_state.load()?.lp_mint,
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Metadata state account storing the creator.
    #[account(
        seeds = [
            LP_METADATA_STATE_SEED.as_bytes(),
            pool_state.key().as_ref(),
        ],
        bump,
    )]
    pub lp_metadata_state: Account<'info, LpMetadataState>,

    /// CHECK: LP mint and metadata authority, signs the metadata CPI.
    #[account(
        seeds = [
            crate::AUTH_SEED.as_bytes(),
        ],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// Metaplex metadata account for the LP mint.
    /// CHECK: owned by the metadata program.
    #[account(
        mut,
        seeds = [
            "metadata".as_bytes(),
            anchor_spl::metadata::ID.as_ref(),
            lp_mint.key().as_ref(),
        ],
        bump,
        seeds::program = anchor_spl::metadata::ID,
    )]
    pub metadata: UncheckedAccount<'info>,

    /// Metaplex token metadata program.
    pub metadata_program: Program<'info, Metadata>,
}

pub fn update_lp_metadata(
    ctx: Context<UpdateLpMetadata>,
    name: String,
    symbol: String,
    uri: String,
) -> Result<()> {
    // Only the creator who created the metadata may update it.
    require!(
        ctx.accounts.creator.key() == ctx.accounts.lp_metadata_state.creator,
        ErrorCode::NotLpMetadataCreator
    );

    let data = DataV2 {
        name,
        symbol,
        uri,
        seller_fee_basis_points: 0,
        creators: None,
        collection: None,
        uses: None,
    };

    let signer_seeds: &[&[u8]] = &[crate::AUTH_SEED.as_bytes(), &[ctx.bumps.authority]];

    metadata::update_metadata_accounts_v2(
        CpiContext::new(
            ctx.accounts.metadata_program.to_account_info(),
            metadata::UpdateMetadataAccountsV2 {
                metadata: ctx.accounts.metadata.to_account_info(),
                update_authority: ctx.accounts.authority.to_account_info(),
            },
        )
        .with_signer(&[signer_seeds]),
        None,       // new_update_authority
        Some(data), // data
        None,       // primary_sale_happened
        None,       // is_mutable
    )?;

    Ok(())
}
