mod program_test;

use anchor_lang::prelude::{ErrorCode, Pubkey};
use program_test::cp_swap_test::CpSwapTest;

use solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::{signature::Keypair, transport::TransportError};
use crate::program_test::program_test_bench::MintType;
use program_test::tools::{assert_anchor_err, assert_ix_err, assert_cp_swap_err};

#[tokio::test]
async fn test_create_registrar() -> Result<(), TransportError> {
    // Arrange
    let mut cp_swap_test = CpSwapTest::start_new().await;

    Ok(())
}
