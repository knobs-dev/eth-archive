use crate::config::{Config, IngestConfig};
use crate::db::DbHandle;
use crate::options::Options;
use crate::parquet_writer::ParquetWriter;
use crate::schema::{Blocks, Logs, Transactions};
use crate::{Error, Result};
use eth_archive_core::eth_client::EthClient;
use eth_archive_core::eth_request::GetBlockByNumber;
use eth_archive_core::retry::Retry;
use eth_archive_core::types::Block;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};

pub struct ParquetWriterRunner {
    db: Arc<DbHandle>,
    cfg: IngestConfig,
    eth_client: Arc<EthClient>,
    block_writer: ParquetWriter<Blocks>,
    transaction_writer: ParquetWriter<Transactions>,
    log_writer: ParquetWriter<Logs>,
    retry: Retry,
}

impl ParquetWriterRunner {
    pub async fn new(options: &Options) -> Result<Self> {
        let config = tokio::fs::read_to_string(&options.cfg_path)
            .await
            .map_err(Error::ReadConfigFile)?;

        let config: Config = toml::de::from_str(&config).map_err(Error::ParseConfig)?;

        let db = DbHandle::new(&config.db)
            .await
            .map_err(|e| Error::CreateDbHandle(Box::new(e)))?;
        let db = Arc::new(db);

        let eth_client =
            EthClient::new(config.ingest.eth_rpc_url.clone()).map_err(Error::CreateEthClient)?;
        let eth_client = Arc::new(eth_client);

        let block_writer = ParquetWriter::new(config.block);
        let transaction_writer = ParquetWriter::new(config.transaction);
        let log_writer = ParquetWriter::new(config.log);

        let retry = Retry::new(config.retry);

        Ok(Self {
            db,
            cfg: config.ingest,
            eth_client,
            block_writer,
            transaction_writer,
            log_writer,
            retry,
        })
    }

    async fn wait_for_next_block(&self, waiting_for: usize) -> Result<Block> {
        loop {
            let block = self.db.get_block(waiting_for as i64).await?;
            if let Some(block) = block {
                return Ok(block);
            } else {
                log::debug!("waiting for next block...");
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    pub async fn run(&self) -> Result<()> {
        let mut block_number = self.initial_sync().await?;

        loop {
            let block = self.wait_for_next_block(block_number).await?;

            self.block_writer.send(vec![block]).await;
            self.db.delete_block(block_number as i64).await?;

            if block_number % 50 == 0 {
                log::info!("deleting block {}", block_number);
            }

            block_number += 1;
        }
    }

    async fn wait_for_start_block_number(&self) -> Result<usize> {
        loop {
            match self.db.get_min_block_number().await? {
                Some(min_num) => return Ok(min_num),
                None => {
                    log::info!("no blocks in database, waiting...");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    pub async fn initial_sync(&self) -> Result<usize> {
        let to_block = self.wait_for_start_block_number().await?;
        log::info!("starting initial sync up to: {}.", to_block);

        let step = self.cfg.http_req_concurrency * self.cfg.block_batch_size;
        for block_num in (0..to_block).step_by(step) {
            log::info!("current block num is {}", block_num);
            let concurrency = self.cfg.http_req_concurrency;
            let batch_size = self.cfg.block_batch_size;
            let start_time = Instant::now();
            let group = (0..concurrency)
                .map(|step| {
                    let eth_client = self.eth_client.clone();
                    let start = block_num + step * batch_size;
                    let end = start + batch_size;
                    async move {
                        self.retry
                            .retry(move || {
                                let eth_client = eth_client.clone();
                                async move {
                                    let batch = (start..end)
                                        .map(|i| GetBlockByNumber { block_number: i })
                                        .collect::<Vec<_>>();
                                    eth_client
                                        .send_batch(&batch)
                                        .await
                                        .map_err(Error::EthClient)
                                }
                            })
                            .await
                    }
                })
                .collect::<Vec<_>>();
            let group = futures::future::join_all(group).await;
            log::info!(
                "downloaded {} blocks in {}ms",
                step,
                start_time.elapsed().as_millis()
            );
            for batch in group {
                let batch = match batch {
                    Ok(batch) => batch,
                    Err(e) => {
                        log::error!("failed batch block req: {:#?}", e);
                        continue;
                    }
                };

                self.block_writer.send(batch).await;
            }
        }
        log::info!("finished initial sync up to block {}", to_block);
        Ok(to_block)
    }
}
