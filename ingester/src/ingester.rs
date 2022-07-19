use crate::config::{Config, IngestConfig};
use crate::db::DbHandle;
use crate::eth_client::EthClient;
use crate::eth_request::GetBlockByNumber;
use crate::options::Options;
use crate::retry::Retry;
use crate::types::Block;
use crate::{Error, Result};
use std::cmp;
use std::sync::Arc;

pub struct Ingester {
    db: Arc<DbHandle>,
    cfg: IngestConfig,
    eth_client: Arc<EthClient>,
}

impl Ingester {
    pub async fn new(options: &Options) -> Result<Self> {
        let config = tokio::fs::read_to_string(&options.cfg_path)
            .await
            .map_err(Error::ReadConfigFile)?;

        let config: Config = toml::de::from_str(&config).map_err(Error::ParseConfig)?;

        let db = DbHandle::new(options, &config.db)
            .await
            .map_err(|e| Error::CreateDbHandle(Box::new(e)))?;
        let db = Arc::new(db);

        let eth_client = EthClient::new(config.ingest.eth_rpc_url.clone())?;
        let eth_client = Arc::new(eth_client);

        Ok(Self {
            db,
            cfg: config.ingest,
            eth_client,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let from_block = self.initial_sync().await?;
        todo!();
    }

    pub async fn initial_sync(&self) -> Result<usize> {
        let from_block = self.db.get_max_block_number().await?.map(|a| a + 1);
        let from_block = match (self.cfg.from_block, from_block) {
            (Some(a), Some(b)) => cmp::max(a, b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => 0,
        };

        let to_block = match self.cfg.to_block {
            Some(to_block) => to_block,
            None => self.eth_client.get_best_block().await? + 1,
        };

        let retry = Retry::new(self.cfg.retry);

        let step = self.cfg.http_req_concurrency * self.cfg.tx_batch_size;
        for block_num in (from_block..to_block).step_by(step) {
            let concurrency = self.cfg.http_req_concurrency;
            let batch_size = self.cfg.tx_batch_size;

            let group = (0..concurrency)
                .map(|step| {
                    let eth_client = self.eth_client.clone();

                    let start = block_num + step * batch_size;
                    let end = start + batch_size;

                    async move {
                        retry
                            .retry(move || {
                                let eth_client = eth_client.clone();
                                async move {
                                    let batch = (start..end)
                                        .map(|i| GetBlockByNumber { block_number: i })
                                        .collect::<Vec<_>>();
                                    eth_client.send_batch(&batch).await
                                }
                            })
                            .await
                    }
                })
                .collect::<Vec<_>>();

            let group = futures::future::join_all(group).await;

            for batch in group {
                let batch = match batch {
                    Ok(batch) => batch,
                    Err(e) => {
                        log::error!("failed batch block req: {:#?}", e);
                        continue;
                    }
                };

                let batch: Arc<[_]> = batch.into();

                retry
                    .retry(move || {
                        let batch = batch.clone();
                        let db = self.db.clone();
                        async move { db.insert_blocks(batch).await }
                    })
                    .await
                    .map_err(Error::InsertBlocks)?;
            }
        }

        Ok(to_block)
    }
}
