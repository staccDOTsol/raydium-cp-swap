#![allow(dead_code)]

            // Shuffle the pools array
            use rand::seq::SliceRandom;
use anchor_client::{Client, Cluster};
use anchor_lang::{AccountDeserialize, AnchorDeserialize};
use anyhow::{format_err, Result};
use arrayref::array_ref;
use clap::Parser;
use configparser::ini::Ini;
use raydium_cp_swap::states::PoolState;
use raydium_cp_swap::{curve::constant_product::ConstantProductCurve, states::AmmConfig};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcTransactionConfig};
use solana_program_runtime::compute_budget;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::program_pack::Pack;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_transaction_status::UiTransactionEncoding;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::ops::Add;
use std::rc::Rc;
use std::str::FromStr;

mod instructions;
use instructions::{amm_instructions::*, rpc};
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
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering;
#[derive(Debug, Parser)]
pub enum RaydiumCpCommands {
    /// Decode transaction logs for a given transaction ID
    DecodeTxLog {
        /// The transaction ID to decode
        tx_id: String,
    },
    SwapBaseOut {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        amount_out_less_fee: u64,
    },
    SwapBaseIn {
        pool_id: Pubkey,
        user_input_token: Pubkey,
        user_input_amount: u64,
    },
    InitializePool {
        mint0: Pubkey,
        mint1: Pubkey,
        init_amount_0: u64,
        init_amount_1: u64,
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
        user_lp_token: Pubkey,
        pool_id: Pubkey,
        lp_token_amount: u64,
    },
    InitializeAmmConfig {
        index: u64,
        token_0_creator_rate: u64,
        token_1_lp_rate: u64,
        token_0_lp_rate: u64,
        token_1_creator_rate: u64,
    },
    Multiswap {
        /// The input token mint address
        input_token: Pubkey,
        /// The desired output token mint address
        output_token: Pubkey,
        /// Amount of input token to swap
        input_amount: u64,
    },
    /// Collect protocol fee
    CollectProtocolFee {
        /// The pool ID
        pool_id: Pubkey,
    },
    /// Collect fund fee
    CollectFundFee {
        /// The pool ID
        amount_0_requested: u64,
        amount_1_requested: u64,
        pool_id: Pubkey,
    },
}


// Define a structure for the pool edge in the graph
#[derive(Clone)]
struct PoolEdge {
    from_token: Pubkey,
    to_token: Pubkey,
    pool_id: Pubkey,
    weight: f64,
    reverse: bool,
}

impl PartialEq for PoolEdge {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for PoolEdge {}

impl PartialOrd for PoolEdge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.weight.partial_cmp(&self.weight)
    }
}

impl Ord for PoolEdge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.weight.partial_cmp(&self.weight).unwrap()
    }
}

fn calculate_weight(rate: f64) -> f64 {
    if rate > 0.0 {
        -rate.ln()
    } else {
        f64::INFINITY
    }
}
use solana_sdk::hash::Hasher;
fn find_best_route(
    rpc_client: &RpcClient,
    pools: &[Pool],
    input_token: Pubkey,
    output_token: Pubkey,
    mut mint_account_owner_cache: &mut HashMap<Pubkey, (Pubkey, u8)>,
    discounted_paths: &mut Vec<usize>,
    pool_config: &ClientConfig,
    payer: &Keypair,
    amount_in: u64,
) -> Result<Vec<PoolEdge>> {
    println!("Starting find_best_route function");
    println!("input_token: {:?}", input_token);
    println!("output_token: {:?}", output_token);
    println!("pools length: {:?}", pools.len());

    // Build the graph with appropriate weights
    let mut graph: HashMap<Pubkey, Vec<PoolEdge>> = HashMap::new();
    let mut token_decimals: HashMap<Pubkey, u8> = HashMap::new();

    // Fetch decimals for all tokens involved
    for pool in pools {
        for token_mint in &[pool.pool.token_0_mint, pool.pool.token_1_mint] {
            if !token_decimals.contains_key(token_mint) {
                let decimals = get_token_decimals(rpc_client, token_mint, mint_account_owner_cache);
                if let Ok(decimals) = decimals {
                    token_decimals.insert(*token_mint, decimals);
                } else {
                    mint_account_owner_cache.insert(*token_mint, (spl_token::id(), 0));
                    println!("Failed to get decimals for token mint: {:?}", token_mint);
                    continue;
                }
            }
        }
    }

    // Build the graph edges
    for pool in pools {
        let token_a = pool.pool.token_0_mint;
        let token_b = pool.pool.token_1_mint;

        let (reserve_a, reserve_b) = match get_pool_reserves(rpc_client, &pool.pool) {
            Ok(reserves) => reserves,
            Err(_) => continue,
        };
        if reserve_a == 0 || reserve_b == 0 {
            continue;
        }
        // Adjust reserves based on decimals
        let decimals_a = token_decimals.get(&token_a).unwrap_or(&0);
        let decimals_b = token_decimals.get(&token_b).unwrap_or(&0);
        let adjusted_reserve_a = reserve_a as f64 / 10_u64.pow(*decimals_a as u32) as f64;
        let adjusted_reserve_b = reserve_b as f64 / 10_u64.pow(*decimals_b as u32) as f64;

        // Calculate rate
        let rate_ab = adjusted_reserve_b / adjusted_reserve_a;

        // Calculate weight
        let weight_ab = calculate_weight(rate_ab);

        // Add edge from token_a to token_b
        graph.entry(token_a).or_default().push(PoolEdge {
            from_token: token_a,
            to_token: token_b,
            pool_id: pool.pubkey,
            weight: weight_ab,
            reverse: false,
        });

        // Similarly for the reverse direction
        let rate_ba = adjusted_reserve_a / adjusted_reserve_b;
        let weight_ba = calculate_weight(rate_ba);

        graph.entry(token_b).or_default().push(PoolEdge {
            from_token: token_b,
            to_token: token_a,
            pool_id: pool.pubkey,
            weight: weight_ba,
            reverse: true,
        });
    }

    // Implement Dijkstra's algorithm to find the shortest (lightest) path
    use std::collections::BinaryHeap;

    struct State {
        cost: f64,
        token: Pubkey,
        path: Vec<PoolEdge>,
        amount_out: u64,
    }

    impl PartialEq for State {
        fn eq(&self, other: &Self) -> bool {
            self.cost == other.cost
        }
    }

    impl PartialOrd for State {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            other.cost.partial_cmp(&self.cost)
        }
    }

    impl Eq for State {}
    impl Ord for State {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other.cost.partial_cmp(&self.cost).unwrap()
        }
    }

    let mut heap = BinaryHeap::new();

    heap.push(State {
        cost: 0.0,
        token: input_token,
        path: Vec::new(),
        amount_out: amount_in,
    });

    while let Some(State { cost, token, path, amount_out }) = heap.pop() {
     
        let mut instructions = Vec::new();
        let mut current_input_token = input_token;
        let mut current_input_amount = amount_in;

        // Iterate through output mints to create ATAs if needed
        let mut output_mints = Vec::new();
        for edge in &path {
            output_mints.push(edge.to_token);
        }
        output_mints.dedup(); // Remove duplicates
        for output_mint in output_mints {
            let output_token_program = mint_account_owner_cache.get(&output_mint).unwrap().0;
            let user_output_token_account = spl_associated_token_account::get_associated_token_address_with_program_id(
                &payer.pubkey(),
                &output_mint,
                &output_token_program,
            );
        
            if rpc_client.get_account(&user_output_token_account).is_err() {
                let create_ata_instr = spl_associated_token_account::instruction::create_associated_token_account(
                    &payer.pubkey(),
                    &payer.pubkey(),
                    &output_mint,
                    &output_token_program,
                );
                // Use the helper function to avoid duplicates
                maybe_add_instruction(&mut instructions, create_ata_instr);
            }
        }

        for (i, edge) in path.iter().enumerate() {
            let pool_state = &pools.iter().find(|p| p.pubkey == edge.pool_id).unwrap().pool;
            let (input_vault, output_vault) = if edge.reverse {
                (pool_state.token_1_vault, pool_state.token_0_vault)
            } else {
                (pool_state.token_0_vault, pool_state.token_1_vault)
            };

            let output_amount = calculate_swap_output(
                rpc_client,
                pool_state,
                mint_account_owner_cache,
                current_input_amount,
                current_input_token,
                edge.to_token,
            )?;

            let minimum_amount_out = amount_with_slippage(output_amount, pool_config.slippage, false);

            prepare_swap_instruction(
                &pool_config,
                edge.pool_id,
                pool_state,
                get_associated_token_address_with_program_id(&payer.pubkey(), &current_input_token, &mint_account_owner_cache.get(&current_input_token).unwrap().0),
                get_associated_token_address_with_program_id(&payer.pubkey(), &edge.to_token, &mint_account_owner_cache.get(&edge.to_token).unwrap().0),
                input_vault,
                output_vault,
                current_input_token,
                edge.to_token,
                current_input_amount,
                minimum_amount_out,
                &mut mint_account_owner_cache,
                &mut instructions,
            )?;
            current_input_token = edge.to_token;
            current_input_amount = output_amount;

            println!("Step {}: Swap {} {} for {} {}", 
                i + 1, 
                current_input_amount, 
                current_input_token, 
                output_amount, 
                edge.to_token
            );
        }
        maybe_add_instruction(&mut instructions, ComputeBudgetInstruction::set_compute_unit_price(333333));
        maybe_add_instruction(&mut instructions, ComputeBudgetInstruction::set_compute_unit_limit(1_400_000));
        println!("Total instructions: {}", instructions.len());

        let signers = vec![&payer];
        let recent_blockhash = rpc_client.get_latest_blockhash()?;
        // Deduplicate instructions
        let mut unique_instructions = Vec::new();
        let mut seen_instructions = HashSet::new();
        
        for instruction in instructions.iter() {
            // Create a hash of the instruction
            let mut hasher = Hasher::default();
            hasher.hash(&bincode::serialize(instruction).unwrap());
            let hash = hasher.result();
        
            if !seen_instructions.contains(&hash) {
                seen_instructions.insert(hash);
                unique_instructions.push(instruction.clone());
            }
        }
        
        instructions = unique_instructions;
        let mut txn = VersionedTransaction::from(Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &signers,
            recent_blockhash,
        ));

        loop {
            let signers = vec![&payer];
            let recent_blockhash = rpc_client.get_latest_blockhash()?;
            txn = VersionedTransaction::from(Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer.pubkey()),
                &signers,
                recent_blockhash,
            ));
            let simulated = rpc_client.simulate_transaction(&txn)?;
            if let Some(err) = simulated.value.err {
                match err {
                    solana_sdk::transaction::TransactionError::DuplicateInstruction(index) => {
                        let needle = instructions[index as usize].clone();
                        let last_index = instructions.iter().enumerate().rev()
                            .find(|(i, x)| *i != index as usize && **x == needle)
                            .map(|(i, _)| i);
                        if let Some(last_index) = last_index {
                            instructions.remove(last_index);
                            break;
                        } else {
                            break;
                        }
                    },
                    _ => {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        let length = base64::encode(&bincode::serialize(&txn)?).len();
        if length > 1232 {
            discounted_paths.push(length);
            continue;
        }
        
        if token == output_token && !path.is_empty() {
            if current_input_amount > amount_in {
                println!("Found profitable path with output amount: {} > input amount: {}, returning early.", current_input_amount, amount_in);
                return Ok(path.clone());
            }
            continue;
            // Start of Selection
            }

            if let Some(edges) = graph.get(&token) {
                for edge in edges {
                    let next_amount_out = calculate_swap_output(
                        rpc_client,
                        &pools.iter().find(|p| p.pubkey == edge.pool_id).unwrap().pool,
                        mint_account_owner_cache,
                        amount_out,
                        token,
                        edge.to_token,
                    )? as u64;

                    let mut new_path = path.clone();
                    new_path.push(edge.clone());
                    
                    // The cost is calculated as the final output of the last leg minus the input amount
                    let profit = next_amount_out as f64 - amount_in as f64;
                    let cost = -profit; // Negative to create a max-heap to maximize profit

                    heap.push(State {
                        cost,
                        token: edge.to_token,
                        path: new_path,
                        amount_out: next_amount_out,
                    });
                }
            }
        }

    println!("No immediate profitable route found, proceeding to find the best available route.");

    Err(format_err!("No profitable route found."))
}
use raydium_cp_swap::curve::CurveCalculator;

fn calculate_swap_output(
    rpc_client: &RpcClient,
    pool: &PoolState,
    mint_account_owner_cache: &mut HashMap<Pubkey, (Pubkey, u8)>,
    amount_in: u64,
    from_token: Pubkey,
    to_token: Pubkey,
) -> Result<u64> {
    let (reserve_0, reserve_1) = get_pool_reserves(rpc_client, pool)?;
    if reserve_0 == 0 || reserve_1 == 0 {
        return Err(format_err!("Pool has zero reserves."));
    }

    let (total_input_amount, total_output_amount) = if from_token == pool.token_0_mint {
        (reserve_0, reserve_1)
    } else {
        (reserve_1, reserve_0)
    };

    // Load AMM config to get fee rates
    let amm_config = rpc_client.get_account(&pool.amm_config)?;
    let amm_config_data = amm_config.data.as_slice();
    let amm_config_state: AmmConfig = AnchorDeserialize::deserialize(&mut &amm_config_data[8..])
        .map_err(|e| format_err!("Failed to deserialize AmmConfig: {}", e))?;

    let (input_token_creator_rate, input_token_lp_rate) = if from_token == pool.token_0_mint {
        (amm_config_state.token_0_creator_rate, amm_config_state.token_0_lp_rate)
    } else {
        (amm_config_state.token_1_creator_rate, amm_config_state.token_1_lp_rate)
    };

    // Use Raydium's CurveCalculator to compute the swap output
    let result = CurveCalculator::swap_base_input(
        u128::from(amount_in),
        u128::from(total_input_amount),
        u128::from(total_output_amount),
        input_token_creator_rate,
        input_token_lp_rate,
    ).ok_or(format_err!("Swap calculation failed"))?;

    let amount_out = u64::try_from(result.destination_amount_swapped)
        .map_err(|_| format_err!("Amount out overflow"))?;

    Ok(amount_out)
}
fn get_token_decimals(
    rpc_client: &RpcClient,
    token_mint: &Pubkey,
    mint_account_owner_cache: &mut HashMap<Pubkey, (Pubkey, u8)>,
) -> Result<u8> {
    if let Some((_, decimals)) = mint_account_owner_cache.get(token_mint) {
        return Ok(*decimals);
    }

    let account = rpc_client.get_account(token_mint)?;
    let mint = Mint::unpack(&account.data)?;
    mint_account_owner_cache.insert(*token_mint, (spl_token::id(), mint.decimals));
    Ok(mint.decimals)
}

fn get_pool_reserves(rpc_client: &RpcClient, pool: &PoolState) -> Result<(u64, u64)> {
    let token_0_vault = rpc_client.get_token_account_balance(&pool.token_0_vault)?;
    let token_1_vault = rpc_client.get_token_account_balance(&pool.token_1_vault)?;
    let reserve_a = token_0_vault.amount.parse::<u64>()?;
    let reserve_b = token_1_vault.amount.parse::<u64>()?;
    Ok((reserve_a, reserve_b))
}
#[derive(Debug, Clone)]
struct Pool {
    pubkey: Pubkey,
    pool: PoolState,
}
    use anyhow::anyhow;
pub fn try_deserialize_unchecked_from_bytes<T: AccountDeserialize>(data: &[u8]) -> Result<T> {
    T::try_deserialize(&mut data.as_ref())
        .map_err(|e| anyhow!("Failed to deserialize account: {}", e))
}
pub fn try_deserialize_unchecked_from_bytes_zc(input: &[u8]) -> Result<PoolState> {
    if input.is_empty() {
        return Err(anyhow::anyhow!("Input data is empty"));
    }
    let pool_state = unsafe {
        let pool_state_ptr = input[8..].as_ptr() as *const PoolState;
        std::ptr::read_unaligned(pool_state_ptr)
    };
    Ok(pool_state)
}
fn fetch_all_pools(rpc_client: &RpcClient, amm_program_id: &Pubkey) -> Result<Vec<Pool>> {
    let mut pools = Vec::new();

    let accounts = rpc_client.get_program_accounts(amm_program_id)?;
    for (pubkey, account) in accounts {
        let pool_data = account.data;
        if let Ok(pool) = try_deserialize_unchecked_from_bytes_zc(&pool_data) {
            pools.push(Pool { pubkey, pool });
        }
    }
    Ok(pools)
}
fn prepare_swap_instruction(
    pool_config: &ClientConfig,
    pool_id: Pubkey,
    pool_state: &PoolState,
    user_input_token: Pubkey,
    user_output_token: Pubkey,
    input_vault: Pubkey,
    output_vault: Pubkey,
    input_token_mint: Pubkey,
    output_token_mint: Pubkey,
    user_input_amount: u64,
    minimum_amount_out: u64,
    mint_account_owner_cache: &mut HashMap<Pubkey, (Pubkey, u8)>,
    instructions: &mut Vec<Instruction>,
    
) -> Result<()> {
    let input_token_program = mint_account_owner_cache.get(&input_token_mint).unwrap().0;
    let output_token_program = mint_account_owner_cache.get(&output_token_mint).unwrap().0;
    let swap_base_in_instr = swap_base_input_instr(
        pool_config,
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
    for instruction in swap_base_in_instr {
        maybe_add_instruction(instructions, instruction);
    }
    Ok(())
}
fn maybe_add_instruction(instructions: &mut Vec<Instruction>, new_instruction: Instruction) {
    // Create a hash of the instruction
    let mut hasher = Hasher::default();
    hasher.hash(&bincode::serialize(&new_instruction).unwrap());
    let hash = hasher.result();

    // Check if we've seen this instruction before
    if !instructions.iter().any(|instr| {
        let mut instr_hasher = Hasher::default();
        instr_hasher.hash(&bincode::serialize(instr).unwrap());
        instr_hasher.result() == hash
    }) {
        instructions.push(new_instruction);
    }
}
fn main() -> Result<()> {

        let opts = Opts::parse();
        let pool_config = load_cfg(&"./client_config.ini".to_string())?;
        let payer = read_keypair_file(&pool_config.payer_path)?;
        let rpc_client = RpcClient::new_with_commitment(pool_config.http_url.clone(), CommitmentConfig::confirmed());
        let mut mint_account_owner_cache: HashMap<Pubkey, (Pubkey, u8)> = HashMap::new();
    
    match opts.command {
        RaydiumCpCommands::Multiswap {
            input_token,
            output_token,
            input_amount,
        } => {
            // Fetch all pools
            let pools = fetch_all_pools(&rpc_client, &pool_config.raydium_cp_program)?;

            // Initialize variables
            let mut discounted_paths: Vec<usize> = Vec::new();

            loop {
                // Shuffle the pools array
                let mut rng = rand::thread_rng();
                let mut pools_shuffled = pools.clone();
                pools_shuffled.shuffle(&mut rng);

                println!("Pools have been shuffled");
                // Find the best route
                let best_route = find_best_route(
                    &rpc_client,
                    &pools_shuffled,
                    input_token,
                    output_token,
                    &mut mint_account_owner_cache,
                    &mut discounted_paths,
                    &pool_config,
                    &payer,
                    input_amount,
                );
                if let Ok(best_route) = best_route {
                    println!("Best route found with {} steps", best_route.len());
                    
                    let mut instructions = Vec::new();
                    let mut current_input_token = input_token;
                    let mut current_input_amount = input_amount;

                // Iterate through output mints to create ATAs if needed
                let mut output_mints = Vec::new();
                for edge in &best_route {
                    output_mints.push(edge.to_token);
                }
                output_mints.dedup(); // Remove duplicates
                for output_mint in output_mints {
                    let output_token_program = mint_account_owner_cache.get(&output_mint).unwrap().0;
                    let user_output_token_account = spl_associated_token_account::get_associated_token_address_with_program_id(
                        &payer.pubkey(),
                        &output_mint,
                        &output_token_program,
                    );
                
                    if rpc_client.get_account(&user_output_token_account).is_err() {
                        let create_ata_instr = spl_associated_token_account::instruction::create_associated_token_account(
                            &payer.pubkey(),
                            &payer.pubkey(),
                            &output_mint,
                            &output_token_program,
                        );
                        // Use the helper function to avoid duplicates
                        maybe_add_instruction(&mut instructions, create_ata_instr);
                    }
                }

                    for (i, edge) in best_route.iter().enumerate() {
                        let pool_state = &pools.iter().find(|p| p.pubkey == edge.pool_id).unwrap().pool;
                        let (input_vault, output_vault) = if edge.reverse {
                            (pool_state.token_1_vault, pool_state.token_0_vault)
                        } else {
                            (pool_state.token_0_vault, pool_state.token_1_vault)
                        };

                        let output_amount = calculate_swap_output(
                            &rpc_client,
                            pool_state,
                            &mut mint_account_owner_cache,
                            current_input_amount,
                            current_input_token,
                            edge.to_token,
                        )?;

                        let minimum_amount_out = amount_with_slippage(output_amount, pool_config.slippage, false);
                            prepare_swap_instruction(
                            &pool_config,
                            edge.pool_id,
                            pool_state,
                            get_associated_token_address_with_program_id(&payer.pubkey(), &current_input_token, &mint_account_owner_cache.get(&current_input_token).unwrap().0),
                            get_associated_token_address_with_program_id(&payer.pubkey(), &edge.to_token, &mint_account_owner_cache.get(&edge.to_token).unwrap().0),
                            input_vault,
                            output_vault,
                            current_input_token,
                            edge.to_token,
                            current_input_amount,
                            minimum_amount_out,
                            &mut mint_account_owner_cache,
                            &mut instructions,
                        )?;

                        current_input_token = edge.to_token;
                        current_input_amount = output_amount;

                        println!("Step {}: Swap {} {} for {} {}", 
                            i + 1, 
                            current_input_amount, 
                            current_input_token, 
                            output_amount, 
                            edge.to_token
                        );
                    }
                    maybe_add_instruction(&mut instructions, ComputeBudgetInstruction::set_compute_unit_price(333333));
                    maybe_add_instruction(&mut instructions, ComputeBudgetInstruction::set_compute_unit_limit(1_400_000));
                    println!("Total instructions: {}", instructions.len());

                    let mut unique_instructions = Vec::new();
                    let mut seen_instructions = HashSet::new();
                    
                    for instruction in instructions.iter() {
                        // Create a hash of the instruction
                        let mut hasher = Hasher::default();
                        hasher.hash(&bincode::serialize(instruction).unwrap());
                        let hash = hasher.result();
                    
                        if !seen_instructions.contains(&hash) {
                            seen_instructions.insert(hash);
                            unique_instructions.push(instruction.clone());
                        }
                    }
                    
                    instructions = unique_instructions;
                                        println!("Total unique instructions: {}", instructions.len());
                    let signers = vec![&payer];
                    let recent_blockhash = rpc_client.get_latest_blockhash()?;
                    let mut txn = VersionedTransaction::from(Transaction::new_signed_with_payer(
                        &instructions,
                        Some(&payer.pubkey()),
                        &signers,
                        recent_blockhash,
                    ));

                    let length = base64::encode(&bincode::serialize(&txn)?).len();
                    println!("Transaction size: {} bytes", length);
                    if length > 1232 {
                        println!("Transaction is too large, skipping");
                        discounted_paths.push(length);
                        continue;
                    }
                    loop {
                        let signers = vec![&payer];
                        let recent_blockhash = rpc_client.get_latest_blockhash()?;
                         txn = VersionedTransaction::from(Transaction::new_signed_with_payer(
                            &instructions,
                            Some(&payer.pubkey()),
                            &signers,
                            recent_blockhash,
                        ));
                       
            let simulated = rpc_client.simulate_transaction(&txn)?;
            println!("Simulation result: {:?}", simulated);
            if let Some(err) = simulated.value.err {
                println!("Simulation failed with error: {:?}", err);
                match err {
                    solana_sdk::transaction::TransactionError::DuplicateInstruction(index) => {
                        println!("Duplicate instruction detected at index: {}", index);
                        let needle = instructions[index as usize].clone();
                        let last_index = instructions.iter().enumerate().rev()
                            .find(|(i, x)| *i != index as usize && **x == needle)
                            .map(|(i, _)| i);
                        if let Some(last_index) = last_index {
                            println!("Index of last duplicate instruction: {}", last_index);
                            instructions.remove(last_index);
                            println!("Removed last duplicate instruction. New instruction count: {}", instructions.len());
                            break;
                        } else {
                            println!("No other duplicate instruction found");
                            break;
                        }
                    },
                    _ => {
                        println!("Unhandled simulation error, skipping this route");
                        break;
                    }
                }
            } else {
                break;
            }
        }
                    let signature = rpc_client.send_transaction(&txn);
                    println!("Transaction signature: {:?}", signature);
                    discounted_paths.push(length);
                } else {
                    println!("No route found, retrying...");
                }
            }
        }
        RaydiumCpCommands::CollectProtocolFee { pool_id } => {
            let pool_account = rpc_client.get_account(&pool_id)?;
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
            let pool_account = rpc_client.get_account(&pool_id)?;
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
            let pool_account = rpc_client.get_account(&pool_id)?;
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
                maybe_add_instruction(&mut instructions, create_ata_ix);
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
            let pool_account = rpc_client.get_account(&pool_id)?;
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
            let pool_account = rpc_client.get_account(&pool_id)?;
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
            let pool_account = rpc_client.get_account(&pool_id)?;
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
