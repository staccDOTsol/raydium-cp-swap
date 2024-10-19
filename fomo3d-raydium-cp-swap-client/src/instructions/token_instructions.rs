use super::super::{read_keypair_file, ClientConfig};
use anchor_client::{Client, Cluster};
use anchor_lang::Key;
use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_program_test::ProgramTestContext;
use solana_sdk::{
    account::WritableAccount,
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
};
use spl_token_2022::{
    extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensionsMut},
    state::{Account, Mint},
};
use spl_token_client::token::ExtensionInitializationParams;
use std::{cell::RefCell, rc::Rc, str::FromStr, sync::Arc};

pub async fn create_and_init_mint_instr(
    ctx: &Arc<RefCell<ProgramTestContext>>,
    token_program: &Pubkey,
    mint_key: &Keypair,
    mint_authority: &Pubkey,
    extension_init_params: Vec<ExtensionInitializationParams>,
    decimals: u8,
) -> Result<Vec<Instruction>> {
    let banks_client = &mut ctx.borrow_mut().banks_client;

    let extension_types = extension_init_params
        .iter()
        .map(|e| e.extension())
        .collect::<Vec<_>>();

    let rent = banks_client.get_rent().await.unwrap();
    let space = ExtensionType::try_calculate_account_len::<Mint>(&extension_types).unwrap();
    let mint_rent = rent.minimum_balance(space);
    let mint_pubkey = mint_key.pubkey();

    let mut instructions = vec![system_instruction::create_account(
        &mint_authority,
        &mint_pubkey,
        mint_rent,
        space as u64,
        token_program,
    )];
    for extension_type in extension_types {
        let instruction = match extension_type {
                ExtensionType::MintCloseAuthority =>
                    spl_token_2022::instruction::initialize_mint_close_authority(
                        token_program,
                        &mint_pubkey,
                        Some(mint_authority),
                    ),
                ExtensionType::DefaultAccountState =>
                    spl_token_2022::extension::default_account_state::instruction::initialize_default_account_state(
                        token_program,
                        &mint_pubkey,
                        &spl_token_2022::state::AccountState::Initialized,
                    ),
                ExtensionType::TransferFeeConfig => spl_token_2022::extension::transfer_fee::instruction::initialize_transfer_fee_config(
                    token_program,
                    &mint_pubkey,
                    Some(mint_authority),
                    Some(mint_authority),
                    100,
                    1_000_000,
                ),
                ExtensionType::InterestBearingConfig => spl_token_2022::extension::interest_bearing_mint::instruction::initialize(
                    token_program,
                    &mint_pubkey,
                    Some(*mint_authority),
                    600,
                ),
                ExtensionType::NonTransferable =>
                    spl_token_2022::instruction::initialize_non_transferable_mint(token_program, &mint_pubkey),
                _ => unimplemented!(),
            };
        instructions.push(instruction.unwrap());
    }
    instructions.push(
        spl_token_2022::instruction::initialize_mint(
            token_program,
            &mint_key.pubkey(),
            mint_authority,
            None,
            decimals,
        )
        .unwrap(),
    );
    Ok(instructions)
}

pub fn create_account_rent_exmpt_instr(
    config: &ClientConfig,
    new_account_key: &Pubkey,
    owner: Pubkey,
    data_size: usize,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let program = client.program(owner)?;
    let instructions = program
        .request()
        .instruction(system_instruction::create_account(
            &program.payer(),
            &new_account_key,
            program
                .rpc()
                .get_minimum_balance_for_rent_exemption(data_size)?,
            data_size as u64,
            &program.id(),
        ))
        .instructions()?;
    Ok(instructions)
}

pub fn create_ata_token_account_instr(
    token_program: Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
) -> Result<Vec<Instruction>> {
    
    Ok(vec![spl_associated_token_account::instruction::create_associated_token_account(
        &owner,
        owner,
        mint,
        &token_program,
    )])
}

pub fn create_and_init_auxiliary_token(
    config: &ClientConfig,
    new_account_key: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    let mint_account = &mut RpcClient::new(config.http_url.to_string()).get_account(&mint)?;
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let (program, space) = if mint_account.owner == spl_token::id() {
        (
            client.program(spl_token::id())?,
            spl_token::state::Account::LEN,
        )
    } else {
        let mut extensions = vec![];
        extensions.push(ExtensionType::ImmutableOwner);
        let mint_state = StateWithExtensionsMut::<Mint>::unpack(mint_account.data_as_mut_slice())?;
        let mint_extension_types = mint_state.get_extension_types()?;
        let mut required_extensions =
            ExtensionType::get_required_init_account_extensions(&mint_extension_types);
        for extension_type in extensions.into_iter() {
            if !required_extensions.contains(&extension_type) {
                required_extensions.push(extension_type);
            }
        }
        let space = ExtensionType::try_calculate_account_len::<Account>(&required_extensions)?;

        (client.program(spl_token_2022::id())?, space)
    };

    let instructions = program
        .request()
        .instruction(system_instruction::create_account(
            &program.payer(),
            &mint,
            program
                .rpc()
                .get_minimum_balance_for_rent_exemption(space)?,
            space as u64,
            &program.id(),
        ))
        .instruction(spl_token_2022::instruction::initialize_immutable_owner(
            &program.id(),
            new_account_key,
        )?)
        .instruction(spl_token_2022::instruction::initialize_account(
            &program.id(),
            new_account_key,
            mint,
            owner,
        )?)
        .instructions()?;
    Ok(instructions)
}

pub fn close_token_account(
    config: &ClientConfig,
    close_account: &Pubkey,
    destination: &Pubkey,
    owner: &Keypair,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let program = client.program(spl_token::id())?;
    let instructions = program
        .request()
        .instruction(spl_token::instruction::close_account(
            &program.id(),
            close_account,
            destination,
            &owner.pubkey(),
            &[],
        )?)
        .signer(owner)
        .instructions()?;
    Ok(instructions)
}

pub fn spl_token_transfer_instr(
    config: &ClientConfig,
    from: &Pubkey,
    to: &Pubkey,
    amount: u64,
    from_authority: &Keypair,
) -> Result<Vec<Instruction>> {
    let payer = read_keypair_file(&config.payer_path);
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
    let program = client.program(spl_token::id())?;
    let instructions = program
        .request()
        .instruction(spl_token::instruction::transfer(
            &program.id(),
            from,
            to,
            &from_authority.pubkey(),
            &[],
            amount,
        )?)
        .signer(from_authority)
        .instructions()?;
    Ok(instructions)
}

pub fn spl_token_mint_to_instr(
    token_program: Pubkey,
    mint: &Pubkey,
    to: &Pubkey,
    amount: u64,
    mint_authority: &Keypair,
) -> Result<Vec<Instruction>> {
    Ok(vec![spl_token_2022::instruction::mint_to(
        &token_program,
        mint,
        to,
        &mint_authority.pubkey(),
        &[&mint_authority.pubkey()],
        amount,
    )?])
}

// pub fn wrap_sol_instr(config: &ClientConfig, amount: u64) -> Result<Vec<Instruction>> {
//     let payer = read_keypair_file(&config.payer_path);
//     let wallet_key = payer.pubkey();
//     let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
//     let wsol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")?;
//     let wsol_ata_account =
//         spl_associated_token_account::get_associated_token_address(&wallet_key, &wsol_mint);
//     // Client.
//     let client = Client::new(url, Rc::new(payer.expect("Failed to get payer keypair")));
//     let program = client.program(spl_token::id())?;

//     let instructions = program
//         .request()
//         .instruction(
//             spl_associated_token_account::instruction::create_associated_token_account_idempotent(
//                 &program.payer(),
//                 &wallet_key,
//                 &wsol_mint,
//                 &program.id(),
//             ),
//         )
//         .instruction(system_instruction::transfer(
//             &wallet_key,
//             &wsol_ata_account,
//             amount,
//         ))
//         .instruction(spl_token::instruction::sync_native(
//             &program.id(),
//             &wsol_ata_account,
//         )?)
//         .instructions()?;
//     Ok(instructions)
// }
