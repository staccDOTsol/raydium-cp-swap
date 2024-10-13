use std::sync::{Arc, RwLock};

use anchor_lang::{prelude::Pubkey, system_program::System, Id};

use anchor_spl::{
    associated_token::{self, AssociatedToken},
    token_interface::TokenAccount,
};
use raydium_cp_swap::{states::{AmmConfig, PoolState, AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_SEED, POOL_VAULT_SEED}, AUTH_SEED};
use solana_sdk::{instruction::AccountMeta, sysvar::instructions};
use spl_token_2022::{extension::StateWithExtensionsMut, state::Mint};

use crate::program_test::program_test_bench::ProgramTestBench;
use anchor_lang::ToAccountMetas;
use solana_program::program_pack::Pack;
use solana_program_test::{processor, BanksClientError, ProgramTest};
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;

use crate::program_test::tools::NopOverride;

use super::{
    program_test_bench::{MintCookie, MintType}, utils::{get_transfer_fee, get_transfer_inverse_fee, TransferFeeInfo}, LoggerWrapper, ProgramOutput
};

#[derive(Debug)]
pub struct AmmCookie {
    pub address: Pubkey,
    pub account: AmmConfig,
}

#[derive(Debug)]
pub struct PoolCookie {
    pub address: Pubkey,
    pub account: PoolState,
}

#[derive(Debug, PartialEq)]
pub struct UserCookie {
    pub key: Keypair,
    pub token_accounts: Vec<Pubkey>,
}

#[allow(dead_code)]
pub struct VoterCookie {
    pub address: Pubkey,
    pub authority: Pubkey,
}

#[allow(dead_code)]
pub struct CpSwapTest {
    pub program_id: Pubkey,
    pub bench: Arc<ProgramTestBench>,
    pub mints: Vec<MintCookie>,
    pub users: Vec<UserCookie>,
}

impl CpSwapTest {
    #[allow(dead_code)]
    pub async fn start_new() -> Self {
        let mut program_test = ProgramTest::new("raydium_cp_swap", raydium_cp_swap::id(), None);
        let (mints, users) = ProgramTestBench::add_mints_and_user_cookies_spl_token(
            &mut program_test,
            MintType::SplToken,
        );
        let program_id = raydium_cp_swap::id();

        let bench = ProgramTestBench::start_new(program_test).await;
        let bench_rc = Arc::new(bench);

        // Setup the environment
        // We need to intercept logs to capture program log output
        let log_filter = "solana_rbpf=trace,\
        solana_runtime::message_processor=debug,\
        solana_runtime::system_instruction_processor=trace,\
        solana_program_test=info";
        let env_logger =
            env_logger::Builder::from_env(env_logger::Env::new().default_filter_or(log_filter))
                .format_timestamp_nanos()
                .build();
        let program_output = Arc::new(RwLock::new(ProgramOutput::default()));
        let _ = log::set_boxed_logger(Box::new(LoggerWrapper {
            inner: env_logger,
            output: program_output.clone(),
        }));

        Self {
            program_id,
            bench: bench_rc,
            mints,
            users,
        }
    }

    #[allow(dead_code)]
    pub async fn initialize_amm_config(
        &self,
        token_1_lp_rate: u64,
        token_0_lp_rate: u64,
        token_0_creator_rate: u64,
        token_1_creator_rate: u64,
    ) -> Result<AmmCookie, BanksClientError> {
        let (amm_config_key, bump) = Pubkey::find_program_address(
            &[AMM_CONFIG_SEED.as_bytes(), &0_u64.to_be_bytes()],
            &self.program_id,
        );

        Ok(AmmCookie {
            address: amm_config_key,
            account: AmmConfig {
                bump,
                disable_create_pool: false,
                index: 0,
                token_1_lp_rate,
                token_0_lp_rate,
                token_0_creator_rate,
                token_1_creator_rate,
                protocol_owner: self.bench.payer.pubkey(),
                fund_owner: self.bench.payer.pubkey(),
                padding: [0; 16],
            },
        })
    }
    #[allow(dead_code)]
    pub async fn initialize_pool(
        &self,
        amm_config_cookie: &AmmCookie,
        token_0_mint: &MintCookie,
        token_1_mint: &MintCookie,
    ) -> Result<PoolCookie, BanksClientError> {
        let (pool_account_key, pool_bump) = Pubkey::find_program_address(
            &[
                POOL_SEED.as_bytes(),
                amm_config_cookie.address.to_bytes().as_ref(),
                token_0_mint.address.to_bytes().as_ref(),
                token_1_mint.address.to_bytes().as_ref(),
            ],
            &self.program_id,
        );
        let (authority, auth_bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &self.program_id);
        let (token_0_vault, _) = Pubkey::find_program_address(
            &[
                POOL_VAULT_SEED.as_bytes(),
                pool_account_key.to_bytes().as_ref(),
                token_0_mint.address.to_bytes().as_ref(),
            ],
            &self.program_id,
        );
        let (token_1_vault, _) = Pubkey::find_program_address(
            &[
                POOL_VAULT_SEED.as_bytes(),
                pool_account_key.to_bytes().as_ref(),
                token_1_mint.address.to_bytes().as_ref(),
            ],
            &self.program_id,
        );
        let (observation_key, _) = Pubkey::find_program_address(
            &[
                OBSERVATION_SEED.as_bytes(),
                pool_account_key.to_bytes().as_ref(),
            ],
            &self.program_id,
        );
    
        let (lp_mint_key, bump) = Pubkey::find_program_address(
            &[
                "pool_lp_mint".as_bytes(),
                pool_account_key.to_bytes().as_ref(),
            ],
            &self.program_id,
        );

        Ok(PoolCookie {
            address: pool_account_key,
            account: PoolState {
                amm_config: amm_config_cookie.address,
                token_0_vault,
                token_1_vault,
                lp_mint: lp_mint_key,
                token_0_mint: token_0_mint.address,
                token_1_mint: token_1_mint.address,
                mint_0_decimals: token_0_mint.decimals,
                mint_1_decimals: token_1_mint.decimals,
                observation_key,
                auth_bump,
                status: 0,
                token_0_program: if token_0_mint.is_token_2022 {
                    spl_token_2022::id()
                } else {
                    spl_token::id()
                },
                token_1_program: if token_1_mint.is_token_2022 {
                    spl_token_2022::id()
                } else {
                    spl_token::id()
                },
                pool_creator: self.bench.payer.pubkey(),
                lp_mint_decimals: 9,
                lp_supply: ,
                protocol_fees_token_0: 0,
                protocol_fees_token_1: 0,
                fund_fees_token_0: 0,
                fund_fees_token_1: 0,
                open_time: ,
                recent_epoch: self.bench.get_clock().await.epoch,
                amm: ,
                padding: [0u64; 31],
            },
        })
    }


    pub async fn get_pool_mints_inverse_fee(
        &self,
        token_mint_0: &Pubkey,
        token_mint_1: &Pubkey,
        post_fee_amount_0: u64,
        post_fee_amount_1: u64,
    ) -> (TransferFeeInfo, TransferFeeInfo) {
        let token_mint_0_data = self.bench.get_account(token_mint_0).await.unwrap();
        let token_mint_1_data = self.bench.get_account(token_mint_1).await.unwrap();
        let epoch = self.bench.get_clock().await.epoch;
        let mut mint0_account = token_mint_0_data.clone();
        let mut mint1_account = token_mint_1_data.clone();
        let mint0_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint0_account.data).unwrap();
        let mint1_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint1_account.data).unwrap();
        (
            TransferFeeInfo {
                mint: *token_mint_0,
                owner: mint0_account.owner,
                transfer_fee: get_transfer_inverse_fee(&mint0_state, post_fee_amount_0, epoch),
            },
            TransferFeeInfo {
                mint: *token_mint_1,
                owner: mint1_account.owner,
                transfer_fee: get_transfer_inverse_fee(&mint1_state, post_fee_amount_1, epoch),
            },
        )
    }

    pub async fn get_pool_mints_transfer_fee(
        &self,
        token_mint_0: &Pubkey,
        token_mint_1: &Pubkey,
        pre_fee_amount_0: u64,
        pre_fee_amount_1: u64,
    ) -> (TransferFeeInfo, TransferFeeInfo) {
        let token_mint_0_data = self.bench.get_account(token_mint_0).await.unwrap();
        let token_mint_1_data = self.bench.get_account(token_mint_1).await.unwrap();
        // let epoch = rpc_client.get_epoch_info().unwrap().epoch;
        let epoch = 0;
        let mut mint0_account = token_mint_0_data.clone();
        let mut mint1_account = token_mint_1_data.clone();
        let mint0_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint0_account.data).unwrap();
        let mint1_state = StateWithExtensionsMut::<Mint>::unpack(&mut mint1_account.data).unwrap();
        (
            TransferFeeInfo {
                mint: *token_mint_0,
                owner: mint0_account.owner,
                transfer_fee: get_transfer_fee(&mint0_state, pre_fee_amount_0, epoch),
            },
            TransferFeeInfo {
                mint: *token_mint_1,
                owner: mint1_account.owner,
                transfer_fee: get_transfer_fee(&mint1_state, pre_fee_amount_1, epoch),
            },
        )
    }

    #[allow(dead_code)]
    pub async fn get_amm_config_account(&self, amm_config: &Pubkey) -> AmmConfig {
        self.bench.get_anchor_account::<AmmConfig>(*amm_config).await
    }

    #[allow(dead_code)]
    pub async fn get_pool_state_account(&self, pool_state: &Pubkey) -> PoolState {
        self.bench.get_anchor_account::<PoolState>(*pool_state).await
    }

    #[allow(dead_code)]
    pub async fn vault_balance(&self, vault: &Pubkey) -> u64 {
        self.bench
            .get_anchor_account::<TokenAccount>(*vault)
            .await
            .amount
    }

    #[allow(dead_code)]
    pub async fn token_balance(&self, token_account: &Pubkey) -> u64 {
        let token_account_data = self.bench.get_account(token_account).await.unwrap();
        let account_info: spl_token::state::Account =
            spl_token::state::Account::unpack_from_slice(token_account_data.data.as_slice()).unwrap();
        account_info.amount
    }
    
    pub fn associated_token_address(&self, address: Pubkey, mint: &MintCookie, token_program_id: &Pubkey,) -> Pubkey {
        spl_associated_token_account::get_associated_token_address_with_program_id(&address, &&mint.address, token_program_id)
    }
}
