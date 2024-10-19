#![allow(dead_code)]

use {
    borsh::BorshDeserialize, mpl_token_metadata::accounts::Metadata, solana_program::{
        borsh1::{get_instance_packed_len, get_packed_len, try_from_slice_unchecked},
        hash::Hash,
        instruction::Instruction,
        program_option::COption,
        program_pack::Pack,
        pubkey::Pubkey,
        stake, system_instruction, system_program,
    }, solana_program_test::{processor, BanksClient, ProgramTest, ProgramTestContext}, solana_sdk::{
        account::{Account as SolanaAccount, WritableAccount},
        clock::{Clock, Epoch},
        compute_budget::ComputeBudgetInstruction,
        signature::{Keypair, Signer},
        transaction::Transaction,
        transport::TransportError,
    }, spl_token_2022::{
        extension::{ExtensionType, StateWithExtensionsOwned},
        native_mint,
        state::{Account, Mint},
    }, std::{convert::TryInto, num::NonZeroU32}
};

pub const FIRST_NORMAL_EPOCH: u64 = 15;
pub const TEST_STAKE_AMOUNT: u64 = 1_500_000_000;
pub const MAX_TEST_VALIDATORS: u32 = 10_000;
pub const DEFAULT_VALIDATOR_STAKE_SEED: Option<NonZeroU32> = NonZeroU32::new(1_010);
pub const DEFAULT_TRANSIENT_STAKE_SEED: u64 = 42;
pub const STAKE_ACCOUNT_RENT_EXEMPTION: u64 = 2_282_880;
const ACCOUNT_RENT_EXEMPTION: u64 = 1_000_000_000; // go with something big to be safe

pub fn program_test() -> ProgramTest {
    let mut program_test = ProgramTest::new("spl_token_2022",
    spl_token_2022::id(),
    processor!(spl_token_2022::processor::Processor::process));
    program_test.prefer_bpf(false);
    program_test.add_program("mpl_token_metadata", mpl_token_metadata::ID, None);

    program_test
}

pub async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> SolanaAccount {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("client error")
        .expect("account not found")
}

#[allow(clippy::too_many_arguments)]
pub async fn create_mint(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    pool_mint: &Keypair,
    manager: &Pubkey,
    decimals: u8,
    extension_types: &[ExtensionType],
) -> Result<(), TransportError> {
    assert!(extension_types.is_empty() || program_id != &spl_token::id());
    let rent = banks_client.get_rent().await.unwrap();
    let space = ExtensionType::try_calculate_account_len::<Mint>(extension_types).unwrap();
    let mint_rent = rent.minimum_balance(space);
    let mint_pubkey = pool_mint.pubkey();

    let mut instructions = vec![system_instruction::create_account(
        &payer.pubkey(),
        &mint_pubkey,
        mint_rent,
        space as u64,
        program_id,
    )];
    for extension_type in extension_types {
        let instruction = match extension_type {
            ExtensionType::MintCloseAuthority =>
                spl_token_2022::instruction::initialize_mint_close_authority(
                    program_id,
                    &mint_pubkey,
                    Some(manager),
                ),
            ExtensionType::DefaultAccountState =>
                spl_token_2022::extension::default_account_state::instruction::initialize_default_account_state(
                    program_id,
                    &mint_pubkey,
                    &spl_token_2022::state::AccountState::Initialized,
                ),
            ExtensionType::TransferFeeConfig => spl_token_2022::extension::transfer_fee::instruction::initialize_transfer_fee_config(
                program_id,
                &mint_pubkey,
                Some(manager),
                Some(manager),
                100,
                1_000_000,
            ),
            ExtensionType::InterestBearingConfig => spl_token_2022::extension::interest_bearing_mint::instruction::initialize(
                program_id,
                &mint_pubkey,
                Some(*manager),
                600,
            ),
            ExtensionType::NonTransferable =>
                spl_token_2022::instruction::initialize_non_transferable_mint(program_id, &mint_pubkey),
            _ => unimplemented!(),
        };
        instructions.push(instruction.unwrap());
    }
    instructions.push(
        spl_token_2022::instruction::initialize_mint(
            program_id,
            &pool_mint.pubkey(),
            manager,
            None,
            decimals,
        )
        .unwrap(),
    );
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, pool_mint],
        *recent_blockhash,
    );
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

pub async fn transfer(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    recipient: &Pubkey,
    amount: u64,
) {
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &payer.pubkey(),
            recipient,
            amount,
        )],
        Some(&payer.pubkey()),
        &[payer],
        *recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();
}

#[allow(clippy::too_many_arguments)]
pub async fn transfer_spl_tokens(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Keypair,
    amount: u64,
    decimals: u8,
) {
    let transaction = Transaction::new_signed_with_payer(
        &[spl_token_2022::instruction::transfer_checked(
            program_id,
            source,
            mint,
            destination,
            &authority.pubkey(),
            &[],
            amount,
            decimals,
        )
        .unwrap()],
        Some(&payer.pubkey()),
        &[payer, authority],
        *recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();
}

#[allow(clippy::too_many_arguments)]
pub async fn create_token_account(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    account: &Keypair,
    pool_mint: &Pubkey,
    authority: &Keypair,
    extensions: &[ExtensionType],
) -> Result<(), TransportError> {
    let rent = banks_client.get_rent().await.unwrap();
    let space = ExtensionType::try_calculate_account_len::<Account>(extensions).unwrap();
    let account_rent = rent.minimum_balance(space);

    let mut instructions = vec![system_instruction::create_account(
        &payer.pubkey(),
        &account.pubkey(),
        account_rent,
        space as u64,
        program_id,
    )];

    for extension in extensions {
        match extension {
            ExtensionType::ImmutableOwner => instructions.push(
                spl_token_2022::instruction::initialize_immutable_owner(
                    program_id,
                    &account.pubkey(),
                )
                .unwrap(),
            ),
            ExtensionType::TransferFeeAmount
            | ExtensionType::MemoTransfer
            | ExtensionType::CpiGuard
            | ExtensionType::NonTransferableAccount => (),
            _ => unimplemented!(),
        };
    }

    instructions.push(
        spl_token_2022::instruction::initialize_account(
            program_id,
            &account.pubkey(),
            pool_mint,
            &authority.pubkey(),
        )
        .unwrap(),
    );

    let mut signers = vec![payer, account];
    for extension in extensions {
        match extension {
            ExtensionType::MemoTransfer => {
                signers.push(authority);
                instructions.push(
                spl_token_2022::extension::memo_transfer::instruction::enable_required_transfer_memos(
                    program_id,
                    &account.pubkey(),
                    &authority.pubkey(),
                    &[],
                )
                .unwrap()
                )
            }
            ExtensionType::CpiGuard => {
                signers.push(authority);
                instructions.push(
                    spl_token_2022::extension::cpi_guard::instruction::enable_cpi_guard(
                        program_id,
                        &account.pubkey(),
                        &authority.pubkey(),
                        &[],
                    )
                    .unwrap(),
                )
            }
            ExtensionType::ImmutableOwner
            | ExtensionType::TransferFeeAmount
            | ExtensionType::NonTransferableAccount => (),
            _ => unimplemented!(),
        }
    }

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &signers,
        *recent_blockhash,
    );
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

pub async fn close_token_account(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    account: &Pubkey,
    lamports_destination: &Pubkey,
    manager: &Keypair,
) -> Result<(), TransportError> {
    let mut transaction = Transaction::new_with_payer(
        &[spl_token_2022::instruction::close_account(
            program_id,
            account,
            lamports_destination,
            &manager.pubkey(),
            &[],
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, manager], *recent_blockhash);
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

pub async fn freeze_token_account(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    account: &Pubkey,
    pool_mint: &Pubkey,
    manager: &Keypair,
) -> Result<(), TransportError> {
    let mut transaction = Transaction::new_with_payer(
        &[spl_token_2022::instruction::freeze_account(
            program_id,
            account,
            pool_mint,
            &manager.pubkey(),
            &[],
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, manager], *recent_blockhash);
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

#[allow(clippy::too_many_arguments)]
pub async fn mint_tokens(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    mint: &Pubkey,
    account: &Pubkey,
    mint_authority: &Keypair,
    amount: u64,
) -> Result<(), TransportError> {
    let transaction = Transaction::new_signed_with_payer(
        &[spl_token_2022::instruction::mint_to(
            program_id,
            mint,
            account,
            &mint_authority.pubkey(),
            &[],
            amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
        &[payer, mint_authority],
        *recent_blockhash,
    );
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

#[allow(clippy::too_many_arguments)]
pub async fn burn_tokens(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    program_id: &Pubkey,
    mint: &Pubkey,
    account: &Pubkey,
    authority: &Keypair,
    amount: u64,
) -> Result<(), TransportError> {
    let transaction = Transaction::new_signed_with_payer(
        &[spl_token_2022::instruction::burn(
            program_id,
            account,
            mint,
            &authority.pubkey(),
            &[],
            amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
        &[payer, authority],
        *recent_blockhash,
    );
    banks_client
        .process_transaction(transaction)
        .await
        .map_err(|e| e.into())
}

pub async fn get_token_balance(banks_client: &mut BanksClient, token: &Pubkey) -> u64 {
    let token_account = banks_client.get_account(*token).await.unwrap().unwrap();
    let account_info = StateWithExtensionsOwned::<Account>::unpack(token_account.data).unwrap();
    account_info.base.amount
}

pub async fn get_metadata_account(banks_client: &mut BanksClient, token_mint: &Pubkey) -> Metadata {
    let (token_metadata, _) = Metadata::find_pda(token_mint);
    let token_metadata_account = banks_client
        .get_account(token_metadata)
        .await
        .unwrap()
        .unwrap();
    Metadata::from_bytes(token_metadata_account.data.as_slice()).unwrap()
}

pub async fn get_token_supply(banks_client: &mut BanksClient, mint: &Pubkey) -> u64 {
    let mint_account = banks_client.get_account(*mint).await.unwrap().unwrap();
    let account_info = StateWithExtensionsOwned::<Mint>::unpack(mint_account.data).unwrap();
    account_info.base.supply
}

pub fn add_mint_account(
    program_test: &mut ProgramTest,
    program_id: &Pubkey,
    mint_key: &Pubkey,
    mint_authority: &Pubkey,
    supply: u64,
) {
    let mut mint_vec = vec![0u8; Mint::LEN];
    let mint = Mint {
        mint_authority: COption::Some(*mint_authority),
        supply,
        decimals: 9,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    Pack::pack(mint, &mut mint_vec).unwrap();
    let stake_pool_mint = SolanaAccount::create(
        ACCOUNT_RENT_EXEMPTION,
        mint_vec,
        *program_id,
        false,
        Epoch::default(),
    );
    program_test.add_account(*mint_key, stake_pool_mint);
}

pub fn add_token_account(
    program_test: &mut ProgramTest,
    program_id: &Pubkey,
    account_key: &Pubkey,
    mint_key: &Pubkey,
    owner: &Pubkey,
) {
    let mut fee_account_vec = vec![0u8; Account::LEN];
    let fee_account_data = Account {
        mint: *mint_key,
        owner: *owner,
        amount: 0,
        delegate: COption::None,
        state: spl_token_2022::state::AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    Pack::pack(fee_account_data, &mut fee_account_vec).unwrap();
    let fee_account = SolanaAccount::create(
        ACCOUNT_RENT_EXEMPTION,
        fee_account_vec,
        *program_id,
        false,
        Epoch::default(),
    );
    program_test.add_account(*account_key, fee_account);
}