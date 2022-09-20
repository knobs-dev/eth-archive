use std::sync::Once;
use std::thread;
use actix_web::rt::{Runtime, spawn};
use actix_web::rt::time::sleep;
use std::time::Duration;
use serde_json::Value;
use serde::Deserialize;
use eth_archive_gateway::{Server, Options};

static INIT: Once = Once::new();

pub fn launch_gateway() {
    INIT.call_once(|| {
        let handle = thread::spawn(|| {
            Runtime::new().unwrap().block_on(async {
                spawn(async {
                    let options = Options {
                        cfg_path: Some("./tests/cfg.toml".to_string()),
                    };
                    Server::run(&options).await.unwrap()
                });
                sleep(Duration::from_secs(1)).await;
            });
        });
        handle.join().unwrap();
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub parquet_block_number: u32,
    pub db_max_block_number: usize,
    pub db_min_block_number: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub number: Option<u32>,
    pub hash: Option<String>,
    pub parent_hash: Option<String>,
    pub nonce: Option<String>,
    pub sha3_uncles: Option<String>,
    pub logs_bloom: Option<String>,
    pub transactions_root: Option<String>,
    pub state_root: Option<String>,
    pub receipts_root: Option<String>,
    pub miner: Option<String>,
    pub difficulty: Option<u32>,
    pub total_difficulty: Option<u32>,
    pub extra_data: Option<String>,
    pub size: Option<u32>,
    pub gas_limit: Option<u32>,
    pub gas_used: Option<u32>,
    pub timestamp: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub block_hash: Option<String>,
    pub block_number: Option<u32>,
    pub source: Option<String>,
    pub gas: Option<u32>,
    pub gas_price: Option<u32>,
    pub hash: Option<String>,
    pub input: Option<String>,
    pub nonce: Option<String>,
    pub dest: Option<String>,
    pub transaction_index: Option<u32>,
    pub value: Option<String>,
    pub kind: Option<u32>,
    pub chain_id: Option<u32>,
    pub v: Option<u32>,
    pub r: Option<String>,
    pub s: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub address: Option<String>,
    pub block_hash: Option<String>,
    pub block_number: Option<u32>,
    pub data: Option<String>,
    pub log_index: Option<u32>,
    pub removed: Option<bool>,
    pub topics: Option<Vec<String>>,
    pub transaction_hash: Option<String>,
    pub transaction_index: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockData {
    pub block: Block,
    pub transactions: Vec<Transaction>,
    pub logs: Vec<Log>,
}

#[derive(Deserialize)]
pub struct QueryResponse {
    pub status: Status,
    pub data: Vec<BlockData>,
}

pub struct Client(reqwest::Client);

impl Client {
    pub fn new() -> Client {
        Client(reqwest::Client::new())
    }

    pub async fn query(&self, args: Value) -> QueryResponse {
        let response = self.0.post("http://127.0.0.1:8080/query")
            .json(&args)
            .send()
            .await
            .unwrap();
        let text = response.text().await.unwrap();
        match serde_json::from_str(&text) {
            Ok(data) => data,
            Err(_) => panic!("Unexpected response body: {}", text)
        }
    }
}
