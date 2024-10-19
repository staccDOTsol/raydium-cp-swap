use solana_banks_client::BanksClientError;
use solana_program_test::{
    tokio::{
        // self, 
        sync::Mutex},
    ProgramTest, ProgramTestContext,
};
use solana_sdk::{
    account::Account,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

// use raydium_cp_swap::{self, accounts as raydium_cp_accounts, instruction as raydium_cp_instructions, states::AMM_CONFIG_SEED, AUTH_SEED};
use std::{cell::RefCell, sync::Arc};
// use anchor_lang::ToAccountMetas;
// use anchor_lang::InstructionData;
// use crate::process_mints;

pub fn program_test() -> ProgramTest {
    let mut program_test = ProgramTest::default();

    program_test.prefer_bpf(true);
    program_test.add_program("raydium_cp_swap", raydium_cp_swap::ID, None);
    program_test.add_program("mpl_token_metadata", mpl_token_metadata::ID, None);

    program_test
}

pub async fn process_transaction(
    ctx: &Arc<RefCell<ProgramTestContext>>,
    instructions: &[Instruction],
    signers: Option<&[&Keypair]>,
) -> Result<(), BanksClientError> {
    let blockhash = {
        let mut context = ctx.borrow_mut();
        context.get_new_latest_blockhash().await?
    };
    let mut context = ctx.borrow_mut();
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&context.payer.pubkey()));

    let mut all_signers = vec![&context.payer];

    if let Some(signers) = signers {
        all_signers.extend_from_slice(signers);
    }

    transaction.sign(&all_signers, blockhash);

    context
        .banks_client
        .process_transaction_with_commitment(
            transaction,
            solana_sdk::commitment_config::CommitmentLevel::Processed,
        )
        .await
}

pub async fn get_account(context: Arc<Mutex<ProgramTestContext>>, pubkey: &Pubkey) -> Account {
    let context = &mut *context.lock().await;
    context
        .banks_client
        .get_account(*pubkey)
        .await
        .unwrap()
        .expect("account not found")
}

pub fn keypair_clone(kp: &Keypair) -> Keypair {
    Keypair::from_bytes(&kp.to_bytes()).expect("failed to copy keypair")
}

// #[tokio::main]
// async fn main() {
//     let context = program_test().start_with_context().await;
//     let ctx = Arc::new(Mutex::new(context));
//     let payer = keypair_clone(&ctx.lock().await.payer);
//     let raydium_cp_swap_program_id = raydium_cp_swap::ID;


//     // INITIALIZE AMM CONFIG
//     let raydium_amm_config_index: u64 = 0;
//     let (amm_config, _bump) = Pubkey::find_program_address(&[AMM_CONFIG_SEED.as_bytes(), &raydium_amm_config_index.to_be_bytes()], &raydium_cp_swap_program_id);

//     let trade_fee_rate = 6666;
//     let protocol_fee_rate = 6666;
//     let fund_fee_rate = 6666;
//     let create_pool_fee = 6666;

//     let create_amm_instruction_args = raydium_cp_instructions::CreateAmmConfig {
//         index:raydium_amm_config_index,
//         trade_fee_rate,
//         protocol_fee_rate,
//         fund_fee_rate,
//         create_pool_fee,
//     }.data();

//     let create_amm_instruction_accounts = raydium_cp_accounts::CreateAmmConfig {
//         owner: payer.pubkey(),
//         amm_config: amm_config,
//         system_program: solana_sdk::system_program::id(),   
//     }.to_account_metas(None);

//     let create_amm_instruction_ix = Instruction {
//         program_id: raydium_cp_swap_program_id,
//         accounts: create_amm_instruction_accounts,
//         data: create_amm_instruction_args,
//     };
//     let context = &mut *ctx.lock().await;

//     process_transaction(context, &[create_amm_instruction_ix], None).await.unwrap();


//     // let random_keypair = Keypair::new();
//     // let (authority, _bump) = Pubkey::find_program_address(&[AUTH_SEED.as_bytes()], &raydium_cp_swap_program_id);
//     // process_mints(&mut context, config, mint0, mint1, &payer).await?;
    
// }

