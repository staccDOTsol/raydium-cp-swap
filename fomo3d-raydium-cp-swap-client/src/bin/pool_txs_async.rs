use anyhow::{Context, Result};
use bs58;
use futures::stream;
use futures::StreamExt;
use rand::seq::SliceRandom;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use std::fs::create_dir_all;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Semaphore;
// use tokio::time::{sleep, Duration};

const SIGNATURE_BYTES: usize = 64;
// highly dependant on your operating system, network speed, and the RPC node you are using and cores.
const MAX_CONCURRENT_REQUESTS: usize = 20;
// use only if RPC is rate limiting
// const RATE_LIMIT_DELAY: Duration = Duration::from_millis(10);
const CP_SWAP_PUBLICKEYS: &str = "fomo3d-raydium-cp-swap-client/cp-swap.txt";
const CP_SWAP_FOLDER: &str = "cp-swap-txs";

#[tokio::main]
async fn main() -> Result<()> {
    // List of RPC URLs
    let rpc_urls = vec![
        "https://burned-young-snowflake.solana-mainnet.quiknode.pro/96e3f49289f987ccdd62dacc40990b20bd21f5ad/",
        "https://skilled-sly-choice.solana-mainnet.quiknode.pro/5db92b766fd9b7ec4cc7e89101473c1d579aa98a/",
        "https://aged-billowing-firefly.solana-mainnet.quiknode.pro/714c2bc2cba308a8c5fe4aee343d31b83b9f42d1/",
        "https://distinguished-dry-sea.solana-mainnet.quiknode.pro/79528918b82740044a48a73406c3139caf8e729d/",
        "https://solitary-yolo-ensemble.solana-mainnet.quiknode.pro/82fe22445068e050d80b27275910aa62734e2520/",
        "https://summer-orbital-gas.solana-mainnet.quiknode.pro/dff876e9e6cb916bc741a761367a91f50ff5dd92/",
        "https://serene-cosmopolitan-arrow.solana-mainnet.quiknode.pro/e5024a662e59587220837fbb749fe7cce477ca09/",
        "https://neat-snowy-bird.solana-mainnet.quiknode.pro/14c0721161ba1af1c4ef91b0a568e2b24edeb9c5/"
    ];

    // Create a directory to store the transaction files
    let txs_dir = Path::new(CP_SWAP_FOLDER);
    if let Err(e) = create_dir_all(&txs_dir) {
        eprintln!("Failed to create directory: {}", e);
    }

    // Read public keys from file
    let pubkeys = read_public_keys(CP_SWAP_PUBLICKEYS, 333).await?;

    // Create a semaphore to limit concurrent requests
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    // Process public keys concurrently
    let results = stream::iter(pubkeys)
        .map(|pubkey| {
            let semaphore = Arc::clone(&semaphore);
            let rpc_urls = rpc_urls.clone();
            let txs_dir = txs_dir.to_path_buf();

            async move {
                // acquire thread permit
                let _permit = semaphore.acquire().await.unwrap();

                // Randomly select an RPC URL for each task
                let rpc_url = rpc_urls
                    .choose(&mut rand::thread_rng())
                    .context("No RPC URLs available")?;
                let client = Arc::new(RpcClient::new(rpc_url.to_string()));

                process_pubkey(client, &pubkey, &txs_dir).await
            }
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .collect::<Vec<_>>()
        .await;

    // Check for any errors
    for result in results {
        if let Err(e) = result {
            eprintln!("Error processing public key: {}", e);
        }
    }

    Ok(())
}

// helper function to read public keys from a file
// returns the vector of public keys
async fn read_public_keys(filename: &str, limit: usize) -> Result<Vec<Pubkey>> {
    let mut pubkeys = Vec::new();
    // Open the cp-swap.txt file for reading
    let file = File::open(filename)
        .await
        .context("Failed to open public keys file")?;
    let reader = BufReader::new(file);

    let mut lines = reader.lines();

    // Read lines asynchronously
    while let Some(line) = lines.next_line().await? {
        if pubkeys.len() >= limit {
            break;
        }
        let pubkey = line.parse::<Pubkey>().context("Invalid public key")?;
        pubkeys.push(pubkey);
    }

    Ok(pubkeys)
}

async fn process_pubkey(client: Arc<RpcClient>, pubkey: &Pubkey, txs_dir: &Path) -> Result<()> {
    let tx_file_path = txs_dir.join(format!("{}.txt", pubkey));
    let mut file = File::create(&tx_file_path)
        .await
        .context("Failed to create transaction file")?;

    // Fetch all signatures
    let signatures = fetch_all_signatures(&client, pubkey).await?;
    println!("Fetched {} signatures for {}", signatures.len(), pubkey);

    // Fetch all transaction data
    let transactions = fetch_all_transactions(&client, &signatures).await?;
    println!("Fetched {} transactions for {}", transactions.len(), pubkey);

    // Write transactions to file
    write_transactions_to_file(&mut file, &transactions).await?;

    file.flush()
        .await
        .context("Failed to flush transaction file")?;
    Ok(())
}

async fn fetch_all_signatures(client: &RpcClient, pubkey: &Pubkey) -> Result<Vec<String>> {
    let mut all_signatures = Vec::new();
    let mut before = None;

    loop {
        let signatures = client
            .get_signatures_for_address_with_config(
                pubkey,
                GetConfirmedSignaturesForAddress2Config {
                    before,
                    until: None,
                    limit: Some(1000),
                    ..GetConfirmedSignaturesForAddress2Config::default()
                },
            )
            .await
            .context("Failed to fetch signatures")?;

        if signatures.is_empty() {
            break;
        }

        all_signatures.extend(signatures.iter().map(|sig| sig.signature.clone()));

        before = signatures
            .last()
            .map(|sig| decode_base58_to_signature(&sig.signature).unwrap());
        if signatures.len() < 1000 {
            break;
        }

        // sleep(RATE_LIMIT_DELAY).await;
    }

    Ok(all_signatures)
}

async fn fetch_all_transactions(
    client: &RpcClient,
    signatures: &[String],
) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>> {
    let mut config = RpcTransactionConfig::default();
    config.encoding = Some(UiTransactionEncoding::JsonParsed);
    config.max_supported_transaction_version = Some(0);

    let config = Arc::new(config);
    let client = Arc::new(client);

    // Process transactions concurrently
    let transactions = stream::iter(signatures)
        .map(|signature| {
            let client = Arc::clone(&client);
            let config = Arc::clone(&config);
            async move {
                if let Some(sig) = decode_base58_to_signature(signature) {
                    match client
                        .get_transaction_with_config(&sig, (*config).clone())
                        .await
                    {
                        Ok(tx) => Some(tx),
                        Err(e) => {
                            eprintln!("Failed to fetch transaction {}: {}", signature, e);
                            None
                        }
                    }
                } else {
                    None
                }
            }
        })
        .buffer_unordered(MAX_CONCURRENT_REQUESTS)
        .filter_map(|tx| async move { tx })
        .collect::<Vec<_>>()
        .await;

    Ok(transactions)
}

async fn write_transactions_to_file(
    file: &mut File,
    transactions: &[EncodedConfirmedTransactionWithStatusMeta],
) -> Result<()> {
    let mut writer = BufWriter::with_capacity(1024 * 1024, file); // 1MB buffer
    let batch_size = 1000; // Adjust based on your needs

    for chunk in transactions.chunks(batch_size) {
        let mut batch = String::with_capacity(chunk.len() * 1024); // Estimate 1KB per transaction
        for tx in chunk {
            batch.push_str(&format!("{:?}\n", tx));
        }
        writer
            .write_all(batch.as_bytes())
            .await
            .context("Failed to write transaction batch to file")?;
    }

    writer
        .flush()
        .await
        .context("Failed to flush transaction data to file")?;
    Ok(())
}

// Helper function to decode a Base58 signature to a Solana Signature
fn decode_base58_to_signature(base58sig: &str) -> Option<Signature> {
    // Decode the Base58 string into a Vec<u8>
    let decoded_bytes = bs58::decode(base58sig).into_vec().ok()?;

    // Ensure the decoded byte length matches SIGNATURE_BYTES
    if decoded_bytes.len() != SIGNATURE_BYTES {
        return None;
    }

    // Convert Vec<u8> to [u8; SIGNATURE_BYTES]
    let mut byte_array = [0u8; SIGNATURE_BYTES];
    byte_array.copy_from_slice(&decoded_bytes);

    // Create a Signature object from the byte array
    Some(Signature::from(byte_array))
}
