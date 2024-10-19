use anchor_client::{Client, Cluster};
use anyhow::Result;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, system_program, sysvar};

use anchor_lang::InstructionData;
use anchor_lang::ToAccountMetas;
use raydium_cp_swap::accounts as raydium_cp_accounts;
use raydium_cp_swap::instruction as raydium_cp_instructions;
use raydium_cp_swap::{
    states::{AMM_CONFIG_SEED, OBSERVATION_SEED, POOL_SEED, POOL_VAULT_SEED},
    AUTH_SEED,
};
use std::rc::Rc;

use super::super::{read_keypair_file, ClientConfig};

pub fn collect_protocol_fee_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    recipient_token_0_account: Pubkey,
    recipient_token_1_account: Pubkey,
    amount_0_requested: u64,
    amount_1_requested: u64,
    amm_config: Pubkey,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let instructions = program
        .request()
        .accounts(raydium_cp_accounts::CollectProtocolFee {
            authority,
            pool_state: pool_id,
            token_0_vault,
            amm_config,
            owner: program.payer(),
            token_1_vault,
            recipient_token_0_account,
            recipient_token_1_account,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            vault_0_mint: token_0_mint,
            vault_1_mint: token_1_mint,
        })
        .args(raydium_cp_instructions::CollectProtocolFee {
            amount_0_requested,
            amount_1_requested,
        })
        .instructions()?;
    Ok(instructions)
}
pub fn collect_fund_fee_instr(
    config: &ClientConfig,
    pool_id: Pubkey,
    amm_config: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    recipient_token_0_account: Pubkey,
    recipient_token_1_account: Pubkey,
    amount_0_requested: u64,
    amount_1_requested: u64,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let program = client.program(config.raydium_cp_program)?;

    let (authority, __bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &program.id());

    let mut instructions = program
        .request()
        .accounts(raydium_cp_accounts::CollectFundFee {
            owner: program.payer(),
            authority,
            pool_state: pool_id,
            amm_config,
            token_0_vault,
            token_1_vault,
            vault_0_mint: token_0_mint,
            vault_1_mint: token_1_mint,
            recipient_token_0_account,
            recipient_token_1_account,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
        })
        .args(raydium_cp_instructions::CollectFundFee {
            amount_0_requested,
            amount_1_requested,
        })
        .instructions()?;

    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    instructions.insert(0, compute_budget_ix);
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(3333333);
    instructions.insert(0, compute_budget_ix);
    Ok(instructions)
}

pub fn initialize_amm_config_instr(
    payer: &Keypair,
    amm_config_index: u64,
    trade_fee_rate: u64,
    protocol_fee_rate: u64,
    fund_fee_rate: u64,
    create_pool_fee: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;

    let (amm_config_key, _bump) = Pubkey::find_program_address(
        &[AMM_CONFIG_SEED.as_bytes(), &amm_config_index.to_be_bytes()],
        &raydium_cp_swap_program_id,
    );

    let create_amm_instruction_args = raydium_cp_instructions::CreateAmmConfig {
        index: amm_config_index,
        trade_fee_rate,
        protocol_fee_rate,
        fund_fee_rate,
        create_pool_fee,
    }
    .data();

    let create_amm_instruction_accounts = raydium_cp_accounts::CreateAmmConfig {
        owner: payer.pubkey(),
        amm_config: amm_config_key,
        system_program: solana_sdk::system_program::id(),
    }
    .to_account_metas(None);

    let create_amm_instruction_ix = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: create_amm_instruction_accounts,
        data: create_amm_instruction_args,
    };

    Ok(vec![create_amm_instruction_ix])
}

pub fn initialize_pool_instr(
    payer: &Keypair,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_0_program: Pubkey,
    token_1_program: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    init_amount_0: u64,
    init_amount_1: u64,
    open_time: u64,
    symbol: String,
    uri: String,
    name: String,
    lp_mint: Pubkey,
    amm_config_index: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;
    let (amm_config_key, __bump) = Pubkey::find_program_address(
        &[AMM_CONFIG_SEED.as_bytes(), &amm_config_index.to_be_bytes()],
        &raydium_cp_swap_program_id,
    );

    let (pool_account_key, __bump) = Pubkey::find_program_address(
        &[
            POOL_SEED.as_bytes(),
            amm_config_key.to_bytes().as_ref(),
            token_0_mint.to_bytes().as_ref(),
            token_1_mint.to_bytes().as_ref(),
        ],
        &raydium_cp_swap_program_id,
    );
    println!("pool_account_key: {}", pool_account_key);
    let (authority, __bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
    let (token_0_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            token_0_mint.to_bytes().as_ref(),
        ],
        &raydium_cp_swap_program_id,
    );
    let (token_1_vault, __bump) = Pubkey::find_program_address(
        &[
            POOL_VAULT_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            token_1_mint.to_bytes().as_ref(),
        ],
        &raydium_cp_swap_program_id,
    );
    let (observation_key, __bump) = Pubkey::find_program_address(
        &[
            OBSERVATION_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
        ],
        &raydium_cp_swap_program_id,
    );

    let (lp_mint_key, __bump) = Pubkey::find_program_address(
        &[
            "pool_lp_mint".as_bytes(),
            pool_account_key.to_bytes().as_ref(),
        ],
        &raydium_cp_swap_program_id,
    );
    let mut instructions = vec![];
    let initialize_args = raydium_cp_instructions::Initialize {
        init_amount_0,
        init_amount_1,
        open_time,
    };
    let initialize_accounts = raydium_cp_accounts::Initialize {
        creator: payer.pubkey(),
        winna_winna_chickum_dinna: payer.pubkey(),
        amm_config: amm_config_key,
        authority,
        pool_state: pool_account_key,
        token_0_mint,
        token_1_mint,
        lp_mint: lp_mint_key,
        creator_token_0: user_token_0_account,
        creator_token_1: user_token_1_account,
        creator_lp_token: spl_associated_token_account::get_associated_token_address(
            &payer.pubkey(),
            &lp_mint_key,
        ),
        token_0_vault,
        token_1_vault,
        observation_state: observation_key,
        token_program: spl_token::id(),
        token_0_program,
        token_1_program,
        associated_token_program: spl_associated_token_account::id(),
        system_program: system_program::id(),
        rent: sysvar::rent::id(),
    };
    let initialize_ix = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: initialize_accounts.to_account_metas(None),
        data: initialize_args.data(),
    };
    instructions.push(initialize_ix);

    // Extend with compute budget instruction

    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    // instructions.insert(0, compute_budget_ix);
    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(3333333);
    // instructions.insert(0, compute_budget_ix);

    // Initialize metadata for the LP token
    let initialize_metadata_instruction_args = raydium_cp_instructions::InitializeMetadata {
        name: name.clone(),
        symbol: symbol.clone(),
        uri: uri.clone(),
    };
    let initialize_metadata_instruction_accounts = raydium_cp_accounts::InitializeMetadata {
        creator: payer.pubkey(),
        authority,
        pool_state: pool_account_key,
        observation_state: observation_key,
        lp_mint: lp_mint_key,
        token_metadata_program: mpl_token_metadata::ID,
        metadata: Pubkey::find_program_address(
            &[
                b"metadata",
                mpl_token_metadata::ID.as_ref(),
                lp_mint_key.as_ref(),
            ],
            &mpl_token_metadata::ID,
        )
        .0,
        system_program: system_program::id(),
        rent: sysvar::rent::id(),
        amm_config: amm_config_key,
    };
    let metadata_instruction = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: initialize_metadata_instruction_accounts.to_account_metas(None),
        data: initialize_metadata_instruction_args.data(),
    };

    instructions.push(metadata_instruction);
    Ok(instructions)
}

pub fn deposit_instr(
    payer: &Keypair,
    pool_id: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_lp_mint: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    user_token_lp_account: Pubkey,
    lp_token_amount: u64,
    maximum_token_0_amount: u64,
    maximum_token_1_amount: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;

    let (authority, __bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
    let mut instructions = vec![];

    let deposit_args = raydium_cp_instructions::Deposit {
        lp_token_amount,
        maximum_token_0_amount: u64::MAX,
        maximum_token_1_amount: u64::MAX,
    };
    let deposit_accounts = raydium_cp_accounts::Deposit {
        owner: payer.pubkey(),
        authority,
        pool_state: pool_id,
        owner_lp_token: user_token_lp_account,
        token_0_account: user_token_0_account,
        token_1_account: user_token_1_account,
        token_0_vault,
        token_1_vault,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        vault_0_mint: token_0_mint,
        vault_1_mint: token_1_mint,
        lp_mint: token_lp_mint,
    };
    let deposit_instruction = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: deposit_accounts.to_account_metas(None),
        data: deposit_args.data(),
    };

    instructions.push(deposit_instruction);

    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    // instructions.insert(0, compute_budget_ix);
    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(3333333);
    // instructions.insert(0, compute_budget_ix);
    Ok(instructions)
}

pub fn withdraw_instr(
    payer: &Keypair,
    pool_id: Pubkey,
    token_0_mint: Pubkey,
    token_1_mint: Pubkey,
    token_lp_mint: Pubkey,
    token_0_vault: Pubkey,
    token_1_vault: Pubkey,
    user_token_0_account: Pubkey,
    user_token_1_account: Pubkey,
    user_token_lp_account: Pubkey,
    lp_token_amount: u64,
    minimum_token_0_amount: u64,
    minimum_token_1_amount: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;

    let (authority, __bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
    let mut instructions = vec![];


    let withdraw_args = raydium_cp_instructions::Withdraw {
        lp_token_amount,
        minimum_token_0_amount,
        minimum_token_1_amount,
    };
    let withdraw_accounts = raydium_cp_accounts::Withdraw {
        owner: payer.pubkey(),
        authority,
        pool_state: pool_id,
        owner_lp_token: user_token_lp_account,
        token_0_account: user_token_0_account,
        token_1_account: user_token_1_account,
        token_0_vault,
        token_1_vault,
        token_program: spl_token::id(),
        token_program_2022: spl_token_2022::id(),
        vault_0_mint: token_0_mint,
        vault_1_mint: token_1_mint,
        memo_program: spl_memo::ID,
        lp_mint: token_lp_mint,
    };
    let withdraw_instruction = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: withdraw_accounts.to_account_metas(None),
        data: withdraw_args.data(),
    };

    instructions.push(withdraw_instruction);

    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    // instructions.insert(0, compute_budget_ix);
    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(3333333);
    // instructions.insert(0, compute_budget_ix);
    Ok(instructions)
}

pub fn swap_base_input_instr(
    payer: &Keypair,
    pool_id: Pubkey,
    amm_config: Pubkey,
    observation_account: Pubkey,
    input_token_account: Pubkey,
    output_token_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    input_token_mint: Pubkey,
    output_token_mint: Pubkey,
    input_token_program: Pubkey,
    output_token_program: Pubkey,
    amount_in: u64,
    minimum_amount_out: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;

    let (authority, __bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
    let mut instructions = vec![];

    let swap_args = raydium_cp_instructions::SwapBaseInput {
        amount_in,
        minimum_amount_out,
    };
    let swap_accounts = raydium_cp_accounts::Swap {
        payer: payer.pubkey(),
        authority,
        amm_config,
        pool_state: pool_id,
        input_token_account,
        output_token_account,
        input_vault,
        output_vault,
        input_token_program,
        output_token_program,
        input_token_mint,
        output_token_mint,
        observation_state: observation_account,
    };
    let swap_instruction = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: swap_accounts.to_account_metas(None),
        data: swap_args.data(),
    };

    instructions.push(swap_instruction);

    Ok(instructions)
}

pub fn swap_base_output_instr(
    payer: &Keypair,
    pool_id: Pubkey,
    amm_config: Pubkey,
    observation_account: Pubkey,
    input_token_account: Pubkey,
    output_token_account: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    input_token_mint: Pubkey,
    output_token_mint: Pubkey,
    input_token_program: Pubkey,
    output_token_program: Pubkey,
    max_amount_in: u64,
    amount_out: u64,
) -> Result<Vec<Instruction>> {
    let raydium_cp_swap_program_id = raydium_cp_swap::ID;

    let (authority, __bump) =
        Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
    let mut instructions = vec![];

    let swap_base_output_args = raydium_cp_instructions::SwapBaseOutput {
        max_amount_in,
        amount_out,
    };
    let swap_base_output_accounts = raydium_cp_accounts::Swap {
        payer: payer.pubkey(),
        authority,
        amm_config,
        pool_state: pool_id,
        input_token_account,
        output_token_account,
        input_vault,
        output_vault,
        input_token_program,
        output_token_program,
        input_token_mint,
        output_token_mint,
        observation_state: observation_account,
    };
    let swap_base_output_instruction = Instruction {
        program_id: raydium_cp_swap_program_id,
        accounts: swap_base_output_accounts.to_account_metas(None),
        data: swap_base_output_args.data(),
    };

    instructions.push(swap_base_output_instruction);

    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    // instructions.insert(0, compute_budget_ix);
    // let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_price(3333333);
    // instructions.insert(0, compute_budget_ix);
    Ok(instructions)
}
