#![allow(dead_code)]
use anchor_client::{Client, Cluster};
use anchor_lang::AccountDeserialize;
use anyhow::{format_err, Result};
use arrayref::array_ref;
use clap::Parser;
use configparser::ini::Ini;
use raydium_cp_swap::states::PoolState;
use raydium_cp_swap::{curve::constant_product::ConstantProductCurve, states::AmmConfig};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_transaction_status::UiTransactionEncoding;
use std::rc::Rc;
use std::str::FromStr;

mod instructions;
use instructions::amm_instructions::*;
use instructions::rpc::*;
use instructions::token_instructions::*;
use instructions::utils::*;
use spl_token_2022::{
    extension::StateWithExtensionsMut,
    state::{Account, Mint},
};

#[derive(Clone, Debug, PartialEq)]
pub struct ClientConfig {
    http_url: String,
    ws_url: String,
    payer_path: String,
    admin_path: String,
    raydium_cp_program: Pubkey,
    slippage: f64,
}

fn load_cfg(client_config: &String) -> Result<ClientConfig> {
    let mut config = Ini::new();
    let _map = config.load(client_config).unwrap();
    let http_url = config.get("Global", "http_url").unwrap();
    if http_url.is_empty() {
        panic!("http_url must not be empty");
    }
    let ws_url = config.get("Global", "ws_url").unwrap();
    if ws_url.is_empty() {
        panic!("ws_url must not be empty");
    }
    let payer_path = config.get("Global", "payer_path").unwrap();
    if payer_path.is_empty() {
        panic!("payer_path must not be empty");
    }
    let admin_path = config.get("Global", "admin_path").unwrap();
    if admin_path.is_empty() {
        panic!("admin_path must not be empty");
    }

    let raydium_cp_program_str = config.get("Global", "raydium_cp_program").unwrap();
    if raydium_cp_program_str.is_empty() {
        panic!("raydium_cp_program must not be empty");
    }
    let raydium_cp_program = Pubkey::from_str(&raydium_cp_program_str).unwrap();
    let slippage = config.getfloat("Global", "slippage").unwrap().unwrap();

    Ok(ClientConfig {
        http_url,
        ws_url,
        payer_path,
        admin_path,
        raydium_cp_program,
        slippage,
    })
}

fn read_keypair_file(s: &str) -> Result<Keypair> {
    solana_sdk::signature::read_keypair_file(s)
        .map_err(|_| format_err!("failed to read keypair from {}", s))
}

#[derive(Debug, Parser)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: RaydiumCpCommands,
}

#[derive(Debug, Parser)]
pub enum RaydiumCpCommands {
    CollectProtocolFee {
        pool_id: Pubkey,
    },
    InitializeAmmConfig {
        index: u64,
        token_0_creator_rate: u64,
        token_1_lp_rate: u64,
        token_0_lp_rate: u64,
        token_1_creator_rate: u64,
    },
    InitializePool {
        mint0: Pubkey,
        mint1: Pubkey,
        init_amount_0: u64,
        init_amount_1: u64,
        #[arg(short, long, default_value_t = 0)]
        open_time: u64,
        symbol: String,
        uri: String,
        name: String,
        amm_config_index: u64,
    },
    Deposit {
        pool_id: Pubkey,
        lp_token_amount: u64,
    },
    Withdraw {
        pool_id: Pubkey,
        user_lp_token: Pubkey,
        lp_token_amount: u64,
    },
    SwapBaseIn {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        user_input_amount: u64,
    },
    SwapBaseOut {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        amount_out_less_fee: u64,
    },
    DecodeTxLog {
        tx_id: String,
    },
    CollectFundFee {
        pool_id: Pubkey,
        amount_0_requested: u64,
        amount_1_requested: u64,
    },
}

fn main() -> Result<()> {
    let client_config = "client_config.ini";
    let pool_config = load_cfg(&client_config.to_string()).unwrap();
    // cluster params.
    let payer = read_keypair_file(&pool_config.payer_path)?;
    // solana rpc client
    let rpc_client = RpcClient::new(pool_config.http_url.to_string());

    // anchor client.
    let anchor_config = pool_config.clone();
    let url = Cluster::Custom(anchor_config.http_url, anchor_config.ws_url);
    let wallet = read_keypair_file(&pool_config.payer_path)?;
    let anchor_client = Client::new(url, Rc::new(wallet));
    let program = anchor_client.program(pool_config.raydium_cp_program)?;

    let opts = Opts::parse();
    match opts.command {
        RaydiumCpCommands::CollectProtocolFee { pool_id } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[200..232]).unwrap(),
            );

            // Extract lp_supply from the pool account data
            let lp_supply =
                u64::from_le_bytes(*<&[u8; 8]>::try_from(&pool_account.data[272..280]).unwrap());

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),

                token_1_vault,
                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };

            let load_pubkeys = vec![
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
            ];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let [amm_config_account, token_0_vault_account, token_1_vault_account, token_0_mint_account, token_1_mint_account] =
                array_ref![rsps, 0, 5];
            // docode account
            let mut token_0_vault_data = token_0_vault_account.clone().unwrap().data;
            let mut token_1_vault_data = token_1_vault_account.clone().unwrap().data;
            let mut token_0_mint_data = token_0_mint_account.clone().unwrap().data;
            let mut token_1_mint_data = token_1_mint_account.clone().unwrap().data;
            let token_0_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_0_vault_data)?;
            let token_1_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_1_vault_data)?;
            let token_0_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_0_mint_data)?;
            let token_1_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_1_mint_data)?;
            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                token_0_vault_info.base.amount,
                token_1_vault_info.base.amount,
            );

            // Create associated token accounts for the recipient (payer in this case)
            let recipient_token_0_account =
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_0_mint,
                );
            let recipient_token_1_account =
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_1_mint,
                );

            // Set requested amounts to u64::MAX to collect all available fees
            let amount_0_requested = u64::MAX;
            let amount_1_requested = u64::MAX;

            let collect_protocol_fee_instr = collect_protocol_fee_instr(
                &pool_config,
                pool_id,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                recipient_token_0_account,
                recipient_token_1_account,
                amount_0_requested,
                amount_1_requested,
                pool_state.amm_config,
            )?;
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &collect_protocol_fee_instr,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );

            println!("Transaction signature: {:?}", txn);
        }
        RaydiumCpCommands::CollectFundFee {
            pool_id,
            amount_0_requested,
            amount_1_requested,
        } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[200..232]).unwrap(),
            );

            // Extract lp_supply from the pool account data
            let lp_supply =
                u64::from_le_bytes(*<&[u8; 8]>::try_from(&pool_account.data[272..280]).unwrap());

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),

                token_1_vault,
                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };
            let recipient_token_0_account =
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_0_mint,
                );
            let recipient_token_1_account =
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_1_mint,
                );
            let collect_fund_fee_instr = collect_fund_fee_instr(
                &pool_config,
                pool_id,
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                recipient_token_0_account,
                recipient_token_1_account,
                amount_0_requested,
                amount_1_requested,
            )?;

            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &collect_fund_fee_instr,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("Collect fund fee transaction signature: {}", signature);
        }
        RaydiumCpCommands::InitializeAmmConfig {
            index,
            token_0_creator_rate,
            token_1_lp_rate,
            token_0_lp_rate,
            token_1_creator_rate,
        } => {
            let initialize_amm_config_instr = initialize_amm_config_instr(
                &pool_config,
                index,
                token_0_creator_rate,
                token_1_lp_rate,
                token_0_lp_rate,
                token_1_creator_rate,
            )?;

            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &initialize_amm_config_instr,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::InitializePool {
            mint0,
            mint1,
            init_amount_0,
            init_amount_1,
            open_time,
            symbol,
            uri,
            name,
            amm_config_index,
        } => {
            let (mint0, mint1, init_amount_0, init_amount_1) = if mint0 > mint1 {
                (mint1, mint0, init_amount_1, init_amount_0)
            } else {
                (mint0, mint1, init_amount_0, init_amount_1)
            };
            let load_pubkeys = vec![mint0, mint1];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let token_0_program = rsps[0].clone().unwrap().owner;
            let token_1_program = rsps[1].clone().unwrap().owner;

            let lp_mint = Keypair::new();
            let initialize_pool_instr = initialize_pool_instr(
                &pool_config,
                mint0,
                mint1,
                token_0_program,
                token_1_program,
                spl_associated_token_account::get_associated_token_address(&payer.pubkey(), &mint0),
                spl_associated_token_account::get_associated_token_address(&payer.pubkey(), &mint1),
                init_amount_0,
                init_amount_1,
                open_time,
                symbol,
                uri,
                name,
                lp_mint.pubkey(),
                amm_config_index,
            )?;

            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &initialize_pool_instr,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::Deposit {
            pool_id,
            lp_token_amount,
        } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[200..232]).unwrap(),
            );

            // Extract lp_supply from the pool account data
            let lp_supply =
                u64::from_le_bytes(*<&[u8; 8]>::try_from(&pool_account.data[272..280]).unwrap());

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),

                token_1_vault,
                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };

            let load_pubkeys = vec![token_0_vault, token_1_vault];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let [token_0_vault_account, token_1_vault_account] = array_ref![rsps, 0, 2];
            let user_token_0 =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &payer.pubkey(),
                    &token_0_mint,
                    &token_0_vault_account.as_ref().unwrap().owner,
                );
            let user_token_1 =
                spl_associated_token_account::get_associated_token_address_with_program_id(
                    &payer.pubkey(),
                    &token_1_mint,
                    &token_1_vault_account.as_ref().unwrap().owner,
                );
            // docode account
            let mut token_0_vault_data = token_0_vault_account.clone().unwrap().data;
            let mut token_1_vault_data = token_1_vault_account.clone().unwrap().data;
            let token_0_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_0_vault_data)?;
            let token_1_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_1_vault_data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                token_0_vault_info.base.amount,
                token_1_vault_info.base.amount,
            );

            // calculate amount
            let results = raydium_cp_swap::curve::CurveCalculator::lp_tokens_to_trading_tokens(
                u128::from(lp_token_amount),
                u128::from(pool_state.lp_supply),
                u128::from(total_token_0_amount),
                u128::from(total_token_1_amount),
                raydium_cp_swap::curve::RoundDirection::Ceiling,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();
            println!(
                "amount_0:{}, amount_1:{}, lp_token_amount:{}",
                results.token_0_amount, results.token_1_amount, lp_token_amount
            );

            // calc with slippage
            let amount_0_with_slippage =
                amount_with_slippage(results.token_0_amount as u64, pool_config.slippage, false);
            let amount_1_with_slippage =
                amount_with_slippage(results.token_1_amount as u64, pool_config.slippage, false);

            // calc with transfer_fee
            let transfer_fee = get_pool_mints_inverse_fee(
                &rpc_client,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                amount_0_with_slippage,
                amount_1_with_slippage,
            );
            println!(
                "transfer_fee_0:{}, transfer_fee_1:{}",
                transfer_fee.0.transfer_fee, transfer_fee.1.transfer_fee
            );
            let amount_0_max = (amount_0_with_slippage as u64)
                .checked_add(transfer_fee.0.transfer_fee)
                .unwrap();
            let amount_1_max = (amount_1_with_slippage as u64)
                .checked_add(transfer_fee.1.transfer_fee)
                .unwrap();
            println!(
                "amount_0_max:{}, amount_1_max:{}",
                amount_0_max, amount_1_max
            );
            let mut instructions = Vec::new();
            // Get account info for user's LP token account
            let user_lp_token = spl_associated_token_account::get_associated_token_address(
                &payer.pubkey(),
                &pool_state.lp_mint,
            );

            // Check if user's LP token account exists, create if not
            if rpc_client.get_account(&user_lp_token).is_err() {
                let create_ata_ix =
                    spl_associated_token_account::instruction::create_associated_token_account(
                        &payer.pubkey(),
                        &payer.pubkey(),
                        &pool_state.lp_mint,
                        &spl_token::id(),
                    );
                instructions.push(create_ata_ix);
            }
            let deposit_instr = deposit_instr(
                &pool_config,
                pool_id,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                pool_state.lp_mint,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                user_token_0,
                user_token_1,
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.lp_mint,
                ),
                lp_token_amount,
                amount_0_max * 10000000,
                amount_1_max * 10000000,
            )?;
            instructions.extend(deposit_instr);
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::Withdraw {
            pool_id,
            user_lp_token,
            lp_token_amount,
        } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::from_str("J9xEwU4Kg6Sx8sGSaWQHyBiJ6NFruaQsc9stvGvEfc3W")?;


            // Extract lp_supply from the pool account data
            let lp_supply = 8800000000;

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                token_1_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),

                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };

            let load_pubkeys = vec![pool_state.token_0_vault, pool_state.token_1_vault];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let [token_0_vault_account, token_1_vault_account] = array_ref![rsps, 0, 2];
            // docode account
            let mut token_0_vault_data = token_0_vault_account.clone().unwrap().data;
            let mut token_1_vault_data = token_1_vault_account.clone().unwrap().data;
            let token_0_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_0_vault_data)?;
            let token_1_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_1_vault_data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                token_0_vault_info.base.amount,
                token_1_vault_info.base.amount,
            );
            // calculate amount
            let results = raydium_cp_swap::curve::CurveCalculator::lp_tokens_to_trading_tokens(
                u128::from(lp_token_amount),
                u128::from(pool_state.lp_supply),
                u128::from(total_token_0_amount),
                u128::from(total_token_1_amount),
                raydium_cp_swap::curve::RoundDirection::Ceiling,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();
            println!(
                "amount_0:{}, amount_1:{}, lp_token_amount:{}",
                results.token_0_amount, results.token_1_amount, lp_token_amount
            );

            // calc with slippage
            let amount_0_with_slippage =
                amount_with_slippage(results.token_0_amount as u64, pool_config.slippage, false);
            let amount_1_with_slippage =
                amount_with_slippage(results.token_1_amount as u64, pool_config.slippage, false);

            let transfer_fee = get_pool_mints_transfer_fee(
                &rpc_client,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                amount_0_with_slippage,
                amount_1_with_slippage,
            );
            println!(
                "transfer_fee_0:{}, transfer_fee_1:{}",
                transfer_fee.0.transfer_fee, transfer_fee.1.transfer_fee
            );
            let amount_0_min = amount_0_with_slippage
                .checked_sub(transfer_fee.0.transfer_fee)
                .unwrap();
            let amount_1_min = amount_1_with_slippage
                .checked_sub(transfer_fee.1.transfer_fee)
                .unwrap();
            println!(
                "amount_0_min:{}, amount_1_min:{}",
                amount_0_min, amount_1_min
            );
            let mut instructions = Vec::new();
           
            let withdraw_instr = withdraw_instr(
                &pool_config,
                pool_id,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                pool_state.lp_mint,
                pool_state.token_0_vault,
                Pubkey::from_str("GBTniBzrhfQp3ohHg1Dqve6eGxJREkRv6eqYtfpGtPH5").unwrap(),
                spl_associated_token_account::get_associated_token_address(
                    &payer.pubkey(),
                    &pool_state.token_0_mint,
                ),
                Pubkey::from_str("35EVhGprq3beVB8oz2uRpzLbu22mZvgwHF9mzshZTFRn").unwrap(),
                user_lp_token,
                lp_token_amount,
                amount_0_min,
                amount_1_min,
            )?;
            instructions.extend(withdraw_instr);
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::SwapBaseIn {
            pool_id,
            user_input_token,
            user_input_amount,
        } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[200..232]).unwrap(),
            );

            // Extract lp_supply from the pool account data
            let lp_supply =
                u64::from_le_bytes(*<&[u8; 8]>::try_from(&pool_account.data[272..280]).unwrap());

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                token_1_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),

                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };
            // load account
            let load_pubkeys = vec![
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                user_input_token,
            ];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let epoch = rpc_client.get_epoch_info().unwrap().epoch;
            let [amm_config_account, token_0_vault_account, token_1_vault_account, token_0_mint_account, token_1_mint_account, user_input_token_account] =
                array_ref![rsps, 0, 6];
            // docode account
            let mut token_0_vault_data = token_0_vault_account.clone().unwrap().data;
            let mut token_1_vault_data = token_1_vault_account.clone().unwrap().data;
            let mut token_0_mint_data = token_0_mint_account.clone().unwrap().data;
            let mut token_1_mint_data = token_1_mint_account.clone().unwrap().data;
            let mut user_input_token_data = user_input_token_account.clone().unwrap().data;
            let amm_config_state = deserialize_anchor_account::<raydium_cp_swap::states::AmmConfig>(
                amm_config_account.as_ref().unwrap(),
            )?;
            let token_0_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_0_vault_data)?;
            let token_1_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_1_vault_data)?;
            let token_0_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_0_mint_data)?;
            let token_1_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_1_mint_data)?;
            let user_input_token_info =
                StateWithExtensionsMut::<Account>::unpack(&mut user_input_token_data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                token_0_vault_info.base.amount,
                token_1_vault_info.base.amount,
            );

            let (
                trade_direction,
                total_input_token_amount,
                total_output_token_amount,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                transfer_fee,
            ) = if user_input_token_info.base.mint == token_0_vault_info.base.mint {
                (
                    raydium_cp_swap::curve::TradeDirection::ZeroForOne,
                    total_token_0_amount,
                    total_token_1_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_1_mint,
                    ),
                    pool_state.token_0_vault,
                    pool_state.token_1_vault,
                    pool_state.token_0_mint,
                    pool_state.token_1_mint,
                    spl_token::id(), //todo fix
                    spl_token::id(), //todo fix
                    get_transfer_fee(&token_0_mint_info, epoch, user_input_amount),
                )
            } else {
                (
                    raydium_cp_swap::curve::TradeDirection::OneForZero,
                    total_token_1_amount,
                    total_token_0_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_0_mint,
                    ),
                    pool_state.token_1_vault,
                    pool_state.token_0_vault,
                    pool_state.token_1_mint,
                    pool_state.token_0_mint,
                    spl_token::id(), //todo fix
                    spl_token::id(), //todo fix
                    get_transfer_fee(&token_1_mint_info, epoch, user_input_amount),
                )
            };
            let (input_token_creator_rate, input_token_lp_rate) = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => (
                    amm_config_state.token_0_creator_rate,
                    amm_config_state.token_0_lp_rate,
                ),
                raydium_cp_swap::curve::TradeDirection::OneForZero => (
                    amm_config_state.token_1_creator_rate,
                    amm_config_state.token_1_lp_rate,
                ),
            };

            let protocol_fee = (input_token_creator_rate + input_token_lp_rate) / 10000 * 2;
            // Take transfer fees into account for actual amount transferred in
            let actual_amount_in = user_input_amount.saturating_sub(transfer_fee);
            let result = raydium_cp_swap::curve::CurveCalculator::swap_base_input(
                u128::from(actual_amount_in),
                u128::from(total_input_token_amount),
                u128::from(total_output_token_amount),
                input_token_creator_rate,
                input_token_lp_rate,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();
            let amount_out = u64::try_from(result.destination_amount_swapped).unwrap();
            let transfer_fee = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => {
                    get_transfer_fee(&token_1_mint_info, epoch, amount_out)
                }
                raydium_cp_swap::curve::TradeDirection::OneForZero => {
                    get_transfer_fee(&token_0_mint_info, epoch, amount_out)
                }
            };
            let amount_received = amount_out.checked_sub(transfer_fee).unwrap();
            // calc mint out amount with slippage
            let minimum_amount_out =
                amount_with_slippage(amount_received, pool_config.slippage, false);

            let mut instructions = Vec::new();
            let create_user_output_token_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &output_token_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_output_token_instr);
            let swap_base_in_instr = swap_base_input_instr(
                &pool_config,
                pool_id,
                pool_state.amm_config,
                pool_state.observation_key,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                user_input_amount,
                minimum_amount_out,
            )?;
            instructions.extend(swap_base_in_instr);
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::SwapBaseOut {
            pool_id,
            user_input_token,
            amount_out_less_fee,
        } => {
            let pool_account = program.rpc().get_account(&pool_id)?;
            let discriminator = &pool_account.data[0..8];
            let token_0_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[72..104]).unwrap(),
            );
            let token_1_vault = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[104..136]).unwrap(),
            );
            let lp_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[136..168]).unwrap(),
            );
            let token_0_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[168..200]).unwrap(),
            );
            let token_1_mint = Pubkey::new_from_array(
                *<&[u8; 32]>::try_from(&pool_account.data[200..232]).unwrap(),
            );

            // Extract lp_supply from the pool account data
            let lp_supply =
                u64::from_le_bytes(*<&[u8; 8]>::try_from(&pool_account.data[272..280]).unwrap());

            println!("LP Mint: {}", lp_mint);
            println!("LP Supply: {}", lp_supply);
            // Create PoolState struct with extracted data
            let pool_state = PoolState {
                amm_config: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                pool_creator: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8..40]).unwrap(),
                ),
                token_0_vault,
                observation_key: Pubkey::new_from_array(
                    *<&[u8; 32]>::try_from(&pool_account.data[8 + 32 * 9..8 + 32 * 10]).unwrap(),
                ),
                token_1_vault,
                lp_mint,
                lp_supply,
                token_0_mint,
                token_1_mint,
                // We don't have access to other fields in the current context,
                // so we'll leave them as default or uninitialized
                ..Default::default()
            };
            // load account
            let load_pubkeys = vec![
                pool_state.amm_config,
                pool_state.token_0_vault,
                pool_state.token_1_vault,
                pool_state.token_0_mint,
                pool_state.token_1_mint,
                user_input_token,
            ];
            let rsps = rpc_client.get_multiple_accounts(&load_pubkeys)?;
            let epoch = rpc_client.get_epoch_info().unwrap().epoch;
            let [amm_config_account, token_0_vault_account, token_1_vault_account, token_0_mint_account, token_1_mint_account, user_input_token_account] =
                array_ref![rsps, 0, 6];
            // docode account
            let mut token_0_vault_data = token_0_vault_account.clone().unwrap().data;
            let mut token_1_vault_data = token_1_vault_account.clone().unwrap().data;
            let mut token_0_mint_data = token_0_mint_account.clone().unwrap().data;
            let mut token_1_mint_data = token_1_mint_account.clone().unwrap().data;
            let mut user_input_token_data = user_input_token_account.clone().unwrap().data;
            let amm_config_state = deserialize_anchor_account::<raydium_cp_swap::states::AmmConfig>(
                amm_config_account.as_ref().unwrap(),
            )?;
            let token_0_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_0_vault_data)?;
            let token_1_vault_info =
                StateWithExtensionsMut::<Account>::unpack(&mut token_1_vault_data)?;
            let token_0_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_0_mint_data)?;
            let token_1_mint_info = StateWithExtensionsMut::<Mint>::unpack(&mut token_1_mint_data)?;
            let user_input_token_info =
                StateWithExtensionsMut::<Account>::unpack(&mut user_input_token_data)?;

            let (total_token_0_amount, total_token_1_amount) = pool_state.vault_amount_without_fee(
                token_0_vault_info.base.amount,
                token_1_vault_info.base.amount,
            );

            let (
                trade_direction,
                total_input_token_amount,
                total_output_token_amount,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                out_transfer_fee,
            ) = if user_input_token_info.base.mint == token_0_vault_info.base.mint {
                (
                    raydium_cp_swap::curve::TradeDirection::ZeroForOne,
                    total_token_0_amount,
                    total_token_1_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_1_mint,
                    ),
                    pool_state.token_0_vault,
                    pool_state.token_1_vault,
                    pool_state.token_0_mint,
                    pool_state.token_1_mint,
                    spl_token::id(), //todo fix
                    spl_token::id(), //todo fix
                    get_transfer_inverse_fee(&token_1_mint_info, epoch, amount_out_less_fee),
                )
            } else {
                (
                    raydium_cp_swap::curve::TradeDirection::OneForZero,
                    total_token_1_amount,
                    total_token_0_amount,
                    user_input_token,
                    spl_associated_token_account::get_associated_token_address(
                        &payer.pubkey(),
                        &pool_state.token_0_mint,
                    ),
                    pool_state.token_1_vault,
                    pool_state.token_0_vault,
                    pool_state.token_1_mint,
                    pool_state.token_0_mint,
                    spl_token::id(), //todo fix
                    spl_token::id(), //todo fix
                    get_transfer_inverse_fee(&token_0_mint_info, epoch, amount_out_less_fee),
                )
            };
            let actual_amount_out = amount_out_less_fee.checked_add(out_transfer_fee).unwrap();
            let (input_token_creator_rate, input_token_lp_rate) = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => (
                    amm_config_state.token_0_creator_rate,
                    amm_config_state.token_0_lp_rate,
                ),
                raydium_cp_swap::curve::TradeDirection::OneForZero => (
                    amm_config_state.token_1_creator_rate,
                    amm_config_state.token_1_lp_rate,
                ),
            };

            let protocol_fee = (input_token_creator_rate + input_token_lp_rate) / 10000 * 2;
            let result = raydium_cp_swap::curve::CurveCalculator::swap_base_output(
                u128::from(actual_amount_out),
                u128::from(total_input_token_amount),
                u128::from(total_output_token_amount),
                input_token_creator_rate,
                input_token_lp_rate,
            )
            .ok_or(raydium_cp_swap::error::ErrorCode::ZeroTradingTokens)
            .unwrap();

            let source_amount_swapped = u64::try_from(result.source_amount_swapped).unwrap();
            let amount_in_transfer_fee = match trade_direction {
                raydium_cp_swap::curve::TradeDirection::ZeroForOne => {
                    get_transfer_inverse_fee(&token_0_mint_info, epoch, source_amount_swapped)
                }
                raydium_cp_swap::curve::TradeDirection::OneForZero => {
                    get_transfer_inverse_fee(&token_1_mint_info, epoch, source_amount_swapped)
                }
            };

            let input_transfer_amount = source_amount_swapped
                .checked_add(amount_in_transfer_fee)
                .unwrap();
            // calc max in with slippage
            let max_amount_in =
                amount_with_slippage(input_transfer_amount, pool_config.slippage, true);
            let mut instructions = Vec::new();
            let create_user_output_token_instr = create_ata_token_account_instr(
                &pool_config,
                spl_token::id(),
                &output_token_mint,
                &payer.pubkey(),
            )?;
            instructions.extend(create_user_output_token_instr);
            let swap_base_in_instr = swap_base_output_instr(
                &pool_config,
                pool_id,
                pool_state.amm_config,
                pool_state.observation_key,
                user_input_token,
                user_output_token,
                input_vault,
                output_vault,
                input_token_mint,
                output_token_mint,
                input_token_program,
                output_token_program,
                max_amount_in,
                amount_out_less_fee,
            )?;
            instructions.extend(swap_base_in_instr);
            let signers = vec![&payer];
            let recent_hash = rpc_client.get_latest_blockhash()?;
            let txn = Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer.pubkey()),
                &signers,
                recent_hash,
            );
            let signature = send_txn(&rpc_client, &txn, true)?;
            println!("{}", signature);
        }
        RaydiumCpCommands::DecodeTxLog { tx_id } => {
            let signature = Signature::from_str(&tx_id)?;
            let tx = rpc_client.get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Json),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            )?;
            let transaction = tx.transaction;
            // get meta
            let meta = if transaction.meta.is_some() {
                transaction.meta
            } else {
                None
            };
            // get encoded_transaction
            let encoded_transaction = transaction.transaction;
            // decode instruction data

            // decode logs
        }
    }
    Ok(())
}
