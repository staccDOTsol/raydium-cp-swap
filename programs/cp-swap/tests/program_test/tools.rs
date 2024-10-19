use anchor_lang::prelude::ERROR_CODE_OFFSET;
use solana_program::instruction::InstructionError;
use solana_program_test::BanksClientError;
use solana_sdk::{signature::Keypair, transaction::TransactionError, transport::TransportError};
use raydium_cp_swap::error::ErrorCode;

pub fn clone_keypair(source: &Keypair) -> Keypair {
    Keypair::from_bytes(&source.to_bytes()).unwrap()
}

/// NOP (No Operation) Override function
#[allow(non_snake_case)]
pub fn NopOverride<T>(_: &mut T) {}

#[allow(dead_code)]
pub fn assert_cp_swap_err(
    banks_client_error: BanksClientError,
    cp_swap_error: ErrorCode,
) {
    let tx_error = banks_client_error.unwrap();

    match tx_error {
        TransactionError::InstructionError(_, instruction_error) => match instruction_error {
            InstructionError::Custom(e) => {
                assert_eq!(e, cp_swap_error as u32 + ERROR_CODE_OFFSET)
            }
            _ => panic!("{:?} Is not InstructionError::Custom()", instruction_error),
        },
        _ => panic!("{:?} Is not InstructionError", tx_error),
    };
}


#[allow(dead_code)]
pub fn assert_anchor_err(
    banks_client_error: BanksClientError,
    anchor_error: anchor_lang::error::ErrorCode,
) {
    let tx_error = banks_client_error.unwrap();

    match tx_error {
        TransactionError::InstructionError(_, instruction_error) => match instruction_error {
            InstructionError::Custom(e) => {
                assert_eq!(e, anchor_error as u32)
            }
            _ => panic!("{:?} Is not InstructionError::Custom()", instruction_error),
        },
        _ => panic!("{:?} Is not InstructionError", tx_error),
    };
}

#[allow(dead_code)]
pub fn assert_ix_err(banks_client_error: BanksClientError, ix_error: InstructionError) {
    let tx_error = banks_client_error.unwrap();

    match tx_error {
        TransactionError::InstructionError(_, instruction_error) => {
            assert_eq!(instruction_error, ix_error);
        }
        _ => panic!("{:?} Is not InstructionError", tx_error),
    };
}

#[allow(dead_code)]
pub fn assert_ix_err_transport(
    banks_client_error: TransportError,
    ix_error: InstructionError
) {
    let tx_error = banks_client_error.unwrap();

    match tx_error {
        TransactionError::InstructionError(_, instruction_error) => {
            assert_eq!(instruction_error, ix_error);
        }
        _ => panic!("{:?} Is not InstructionError", tx_error),
    };
}