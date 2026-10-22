//! Exercise disk-backed chain activation by withholding and then releasing one block.

use antithesis_sdk::random::random_choice;
use bitcoin::{
    absolute, block, consensus::encode::serialize_hex, transaction, Amount, Block, BlockHash,
    CompactTarget, ScriptBuf, Transaction, TxIn, TxMerkleNode, TxOut,
};
use bitcoin_antithesis_workload::{
    assert_mempool_metrics, assert_reorg_metrics, assert_wallet_metrics, create_client,
    get_all_nodes, random_node, random_range, random_u64, Client,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
// Around the four-block read-ahead window and the 32-block reorg batch boundary.
const CHAIN_LENGTHS: [usize; 7] = [2, 4, 5, 6, 32, 33, 65];

#[derive(Clone, Copy, Serialize)]
enum Scenario {
    First,
    Middle,
    Reorg,
    ReorgTip,
}
const SCENARIOS: [Scenario; 4] = [
    Scenario::First,
    Scenario::Middle,
    Scenario::Reorg,
    Scenario::ReorgTip,
];

impl Scenario {
    fn parameters(self, count: usize) -> (usize, usize) {
        // (fork depth, withheld block index)
        match self {
            Self::First => (0, 0),
            Self::Middle => (0, count / 2),
            Self::Reorg => (count - 1, 0),
            Self::ReorgTip => (count - 1, count - 1),
        }
    }
}

#[derive(Deserialize)]
struct HeaderInfo {
    hash: BlockHash,
    height: u64,
    time: u32,
    bits: String,
    previousblockhash: Option<BlockHash>,
    confirmations: i64,
    #[serde(rename = "nTx")]
    n_tx: u64,
}

fn header_info(client: &Client, hash: BlockHash) -> Result<HeaderInfo> {
    Ok(client.call("getblockheader", &[json!(hash)])?)
}

fn build_chain(client: &Client, count: usize, depth: usize) -> Result<(BlockHash, Vec<Block>)> {
    let info: Value = client.call("getblockchaininfo", &[])?;
    if info["chain"] != "regtest" {
        return Err("requires regtest".into());
    }
    let previous_tip = serde_json::from_value(info["bestblockhash"].clone())?;
    let mut tip = header_info(client, previous_tip)?;
    for _ in 0..depth {
        tip = header_info(
            client,
            tip.previousblockhash.ok_or("chain too short for reorg")?,
        )?;
    }
    let bits = CompactTarget::from_consensus(u32::from_str_radix(&tip.bits, 16)?);
    if bits.to_consensus() != 0x207fffff {
        return Err("requires default regtest difficulty".into());
    }
    // Track actual median-time-past, including earlier blocks with future timestamps.
    let mut times = vec![tip.time];
    let mut previous = tip.previousblockhash;
    while times.len() < 11 {
        let Some(hash) = previous else { break };
        let header = header_info(client, hash)?;
        times.push(header.time);
        previous = header.previousblockhash;
    }
    times.reverse();
    let now = u32::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    let salt = random_u64().to_le_bytes();
    let mut prev_blockhash = tip.hash;
    let mut blocks = Vec::with_capacity(count);
    for offset in 1..=count {
        let mut sorted_times = times.clone();
        sorted_times.sort_unstable();
        let time = now.max(sorted_times[sorted_times.len() / 2] + 1);
        // Zero-subsidy coinbase: no wallet, mempool, or halving dependency.
        let coinbase = Transaction {
            version: transaction::Version::ONE,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                script_sig: ScriptBuf::builder()
                    .push_int((tip.height + offset as u64) as i64)
                    .push_slice(salt)
                    .into_script(),
                ..TxIn::default()
            }],
            output: vec![TxOut {
                value: Amount::ZERO,
                script_pubkey: ScriptBuf::new_op_return([]),
            }],
        };
        let mut block = Block {
            header: block::Header {
                version: block::Version::from_consensus(0x20000000),
                prev_blockhash,
                merkle_root: TxMerkleNode::from_raw_hash(coinbase.compute_txid().to_raw_hash()),
                time,
                bits,
                nonce: 0,
            },
            txdata: vec![coinbase],
        };
        while block.header.validate_pow(block.header.target()).is_err() {
            block.header.nonce += 1;
        }
        prev_blockhash = block.block_hash();
        blocks.push(block);
        times.push(time);
        if times.len() > 11 {
            times.remove(0);
        }
    }
    Ok((previous_tip, blocks))
}

fn submit_block(client: &Client, block: &Block) -> Result<()> {
    let result: Option<String> = client.call("submitblock", &[json!(serialize_hex(block))])?;
    match result.as_deref() {
        None | Some("inconclusive") => {} // Expected while an ancestor body is missing.
        Some(reason) => return Err(format!("submitblock {}: {reason}", block.block_hash()).into()),
    }
    // nTx confirms a recorded disk position without reading the body back into cache.
    if header_info(client, block.block_hash())?.n_tx != 1 {
        return Err(format!("body not recorded for {}", block.block_hash()).into());
    }
    Ok(())
}

#[derive(Serialize)]
struct Outcome {
    covered: bool,
    reorg: bool,
    tip: BlockHash,
}

fn exercise(client: &Client, count: usize, scenario: Scenario, reverse: bool) -> Result<Outcome> {
    let (depth, gap) = scenario.parameters(count);
    let (previous_tip, blocks) = build_chain(client, count, depth)?;
    for block in &blocks {
        client.call::<Value>("submitheader", &[json!(serialize_hex(&block.header))])?;
    }
    let mut order: Vec<usize> = (0..count).filter(|&i| i != gap).collect();
    if reverse {
        order.reverse();
    } else {
        for i in (1..order.len()).rev() {
            order.swap(i, random_range((i + 1) as u64) as usize);
        }
    }
    for index in order {
        submit_block(client, &blocks[index])?;
    }
    let missing = header_info(client, blocks[gap].block_hash())?;
    let tip = blocks.last().unwrap().block_hash();
    let last = header_info(client, tip)?;
    let prefix_ready = !matches!(scenario, Scenario::Middle)
        || header_info(client, blocks[gap - 1].block_hash())?.confirmations > 0;
    let staged = missing.n_tx == 0 && last.confirmations == -1 && prefix_ready;
    let old_tip_active = depth > 0 && header_info(client, previous_tip)?.confirmations > 0;
    antithesis_sdk::assert_always!(
        missing.n_tx != 0 || last.confirmations == -1,
        "Descendants cannot connect without their ancestor body",
        &json!({ "tip": tip, "gap": gap, "scenario": scenario,
            "missing_block": missing.hash, "missing_block_txs": missing.n_tx,
            "tip_confirmations": last.confirmations })
    );
    // ReorgTip supplies the tip in memory; the other cases unlock stored descendants.
    submit_block(client, &blocks[gap])?;
    let connected = header_info(client, tip)?.confirmations > 0;
    let reorg = old_tip_active && header_info(client, previous_tip)?.confirmations == -1;
    Ok(Outcome {
        covered: staged && connected,
        reorg,
        tip,
    })
}

fn main() {
    antithesis_sdk::antithesis_init();
    let nodes = get_all_nodes();
    let node = random_node(&nodes);
    let count = *random_choice(&CHAIN_LENGTHS).expect("CHAIN_LENGTHS is non-empty");
    let scenario = *random_choice(&SCENARIOS).expect("SCENARIOS is non-empty");
    let reverse = random_range(2) == 0;
    let client = match create_client(node) {
        Ok(client) => client,
        Err(e) => {
            eprintln!(
                "[out_of_order_blocks] {} client creation failed: {e}",
                node.host
            );
            return;
        }
    };
    let result = exercise(&client, count, scenario, reverse);
    if let Err(e) = &result {
        eprintln!("[out_of_order_blocks] {}: {e}", node.host);
    }
    // Competing chains and injected faults can prevent connection: use coverage targets.
    let covered = result.as_ref().is_ok_and(|o| o.covered);
    let reorg = covered && result.as_ref().is_ok_and(|o| o.reorg);
    let middle_gap = covered && matches!(scenario, Scenario::Middle) && count > 2;
    let provided_tip = reorg && matches!(scenario, Scenario::ReorgTip);
    let details = json!({ "node": node.host, "blocks": count, "scenario": scenario,
        "reverse": reverse, "outcome": result.as_ref().ok(),
        "error": result.as_ref().err().map(|e| e.to_string()) });
    println!("[out_of_order_blocks] {details}");
    antithesis_sdk::assert_sometimes!(
        covered,
        "Out-of-order blocks connect after filling a gap",
        &details
    );
    antithesis_sdk::assert_sometimes!(
        middle_gap,
        "Stored descendants connect after filling a middle gap",
        &details
    );
    antithesis_sdk::assert_sometimes!(
        reorg,
        "A stored out-of-order branch replaces the active chain",
        &details
    );
    antithesis_sdk::assert_sometimes!(
        provided_tip,
        "Reorg connects stored blocks and a caller-provided tip",
        &details
    );
    // Deep reorgs force a batch boundary; extensions can yield after each block.
    antithesis_sdk::assert_sometimes!(
        reorg && count > 32,
        "Stored-block reorg crosses the 32-block batch boundary",
        &details
    );

    assert_reorg_metrics(&client, "after_out_of_order_blocks");
    assert_mempool_metrics(&client, "after_out_of_order_blocks");
    assert_wallet_metrics(&client, "after_out_of_order_blocks");
}
