use std::{env, time::Duration};

use electrsd::corepc_node::{self, Node as BitcoinD};
use electrsd::ElectrsD;
use ldk_node::bitcoin::{
    address::NetworkUnchecked, Address, Amount, Network as BitcoinNetwork, Txid,
};
use reqwest::StatusCode;
use saturn::{
    app::AppConfig,
    domain::entities::{PaymentFinality, SettlementProof},
    payments::build_payment_adapters,
};
use serde::Deserialize;
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires downloaded bitcoind/electrs binaries and a live regtest environment"]
async fn ldk_onchain_adapter_verifies_real_regtest_transaction() {
    let (bitcoind, electrsd) = setup_bitcoind_and_electrsd();
    let rpc = &bitcoind.client;
    let _ = rpc.create_wallet("saturn_regtest_wallet");
    let _ = rpc.load_wallet("saturn_regtest_wallet");
    generate_blocks_and_wait(rpc, &electrsd, 101).await;

    let mut config = AppConfig::for_tests();
    config.lightning_backend = "mock".into();
    config.onchain_backend = "ldk".into();
    config.lightning_ldk_network = "regtest".into();
    config.lightning_ldk_rgs_url = None;
    config.lightning_ldk_esplora_url = format!(
        "http://{}",
        electrsd
            .esplora_url
            .as_ref()
            .expect("electrsd should expose esplora")
    );
    config.lightning_ldk_storage_dir = env::temp_dir()
        .join(format!("saturn-ldk-regtest-{}", Uuid::new_v4()))
        .display()
        .to_string();

    let (_lightning_adapter, onchain_adapter) =
        build_payment_adapters(&config).expect("ldk adapters should start");
    let address = onchain_adapter
        .new_address(Uuid::new_v4())
        .await
        .expect("ldk on-chain adapter should derive an address");
    let parsed_address = address
        .parse::<Address<NetworkUnchecked>>()
        .expect("address should parse")
        .require_network(BitcoinNetwork::Regtest)
        .expect("address should target regtest");

    let txid = rpc
        .send_to_address(&parsed_address, Amount::from_sat(21_000))
        .expect("bitcoind should fund the LDK address")
        .0
        .parse::<Txid>()
        .expect("txid should parse");
    wait_for_esplora_tx(&config.lightning_ldk_esplora_url, &txid).await;

    let vout = find_output_index(&config.lightning_ldk_esplora_url, &txid, &address, 21_000)
        .await
        .expect("funding transaction should contain the expected output");
    let pending_proof = SettlementProof::OnChain {
        txid: txid.to_string(),
        vout,
        amount_sats: 21_000,
        confirmations: 0,
    };

    let pending = onchain_adapter
        .verify_settlement(&pending_proof, &address, 21_000, 1)
        .await
        .expect("unconfirmed regtest transaction should normalize");
    assert_eq!(pending.finality, PaymentFinality::Pending);

    generate_blocks_and_wait(rpc, &electrsd, 1).await;

    let confirmed = onchain_adapter
        .verify_settlement(&pending_proof, &address, 21_000, 1)
        .await
        .expect("confirmed regtest transaction should verify");
    assert_eq!(confirmed.finality, PaymentFinality::Confirmed);
    assert_eq!(
        confirmed.normalized_proof,
        SettlementProof::OnChain {
            txid: txid.to_string(),
            vout,
            amount_sats: 21_000,
            confirmations: 1,
        }
    );
    assert!(confirmed.settled_at.timestamp() > 0);
}

fn setup_bitcoind_and_electrsd() -> (BitcoinD, ElectrsD) {
    let bitcoind_exe = env::var("BITCOIND_EXE")
        .ok()
        .or_else(|| corepc_node::downloaded_exe_path().ok())
        .expect("set BITCOIND_EXE or enable corepc-node downloads");
    let mut bitcoind_conf = corepc_node::Conf::default();
    bitcoind_conf.network = "regtest";
    bitcoind_conf.args.push("-rest");
    let bitcoind = BitcoinD::with_conf(bitcoind_exe, &bitcoind_conf)
        .expect("bitcoind should start for regtest");

    let electrs_exe = env::var("ELECTRS_EXE")
        .ok()
        .or_else(electrsd::downloaded_exe_path)
        .expect("set ELECTRS_EXE or enable electrsd downloads");
    let mut electrsd_conf = electrsd::Conf::default();
    electrsd_conf.http_enabled = true;
    electrsd_conf.network = "regtest";
    let electrsd = ElectrsD::with_conf(electrs_exe, &bitcoind, &electrsd_conf)
        .expect("electrsd should start for regtest");

    (bitcoind, electrsd)
}

async fn generate_blocks_and_wait(
    rpc: &corepc_node::Client,
    electrsd: &ElectrsD,
    blocks: usize,
) {
    let start_height = rpc
        .get_blockchain_info()
        .expect("blockchain info should be available")
        .blocks as u32;
    let address = rpc
        .get_new_address(None, None)
        .expect("wallet address")
        .0
        .parse::<Address<NetworkUnchecked>>()
        .expect("mining address should parse")
        .assume_checked();
    rpc.generate_to_address(blocks, &address)
        .expect("regtest blocks should mine");

    let esplora_base_url = format!(
        "http://{}",
        electrsd
            .esplora_url
            .as_ref()
            .expect("electrsd should expose esplora")
    );
    wait_for_tip_height(&esplora_base_url, start_height.saturating_add(blocks as u32)).await;
}

async fn wait_for_tip_height(esplora_base_url: &str, target_height: u32) {
    let client = reqwest::Client::new();
    let url = format!("{}/blocks/tip/height", esplora_base_url);

    for _ in 0..120 {
        let response = client
            .get(&url)
            .send()
            .await
            .expect("tip height poll should succeed");
        let height: u32 = response
            .text()
            .await
            .expect("tip height body should be readable")
            .trim()
            .parse()
            .expect("tip height should parse");
        if height >= target_height {
            return;
        }
        sleep(Duration::from_millis(250)).await;
    }

    panic!("timed out waiting for esplora tip height to reach {target_height}");
}

async fn wait_for_esplora_tx(esplora_base_url: &str, txid: &Txid) {
    let client = reqwest::Client::new();
    let url = format!("{}/tx/{txid}", esplora_base_url);

    for _ in 0..120 {
        let response = client.get(&url).send().await.expect("tx poll should succeed");
        if response.status() == StatusCode::OK {
            return;
        }
        sleep(Duration::from_millis(250)).await;
    }

    panic!("timed out waiting for esplora to index transaction {txid}");
}

async fn find_output_index(
    esplora_base_url: &str,
    txid: &Txid,
    expected_address: &str,
    expected_value: u64,
) -> Option<u32> {
    let client = reqwest::Client::new();
    let url = format!("{}/tx/{txid}", esplora_base_url);
    let transaction: EsploraTransaction = client
        .get(&url)
        .send()
        .await
        .expect("transaction query should succeed")
        .json()
        .await
        .expect("transaction response should decode");

    transaction
        .vout
        .iter()
        .enumerate()
        .find(|(_, output)| {
            output.scriptpubkey_address.as_deref() == Some(expected_address)
                && output.value == expected_value
        })
        .map(|(index, _)| index as u32)
}

#[derive(Debug, Deserialize)]
struct EsploraTransaction {
    vout: Vec<EsploraTransactionOutput>,
}

#[derive(Debug, Deserialize)]
struct EsploraTransactionOutput {
    value: u64,
    scriptpubkey_address: Option<String>,
}
