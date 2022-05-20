use eth_archive::eth_client::EthClient;
use eth_archive::eth_request::{GetBlockByNumber, GetLogs};
use eth_archive::parquet_writer::ParquetWriter;
use eth_archive::schema::{Blocks, Logs, Transactions};
use std::mem;
use std::sync::Arc;
use std::time::Instant;

use eth_archive::retry::retry;

#[tokio::main]
async fn main() {
    let client = EthClient::new("https://rpc.ankr.com/eth").unwrap();
    let client = Arc::new(client);

    let block_writer: ParquetWriter<Blocks> = ParquetWriter::new("data/block/block", 1_000_000);
    let tx_writer: ParquetWriter<Transactions> = ParquetWriter::new("data/tx/tx", 3_300_000);
    let log_writer: ParquetWriter<Logs> = ParquetWriter::new("data/log/log", 3_300_000);

    let block_range = 10_000_000..14_000_000;

    let block_tx_job = tokio::spawn({
        let client = client.clone();
        async move {
            const BATCH_SIZE: usize = 100;
            const STEP: usize = 100;

            for block_num in block_range.step_by(STEP * BATCH_SIZE) {
                let start = Instant::now();
                let group = (0..STEP)
                    .map(|step| {
                        let client = client.clone();
                        retry(move || {
                            let client = client.clone();
                            let start = block_num + step * BATCH_SIZE;
                            let end = start + BATCH_SIZE;

                            let batch = (start..end)
                                .map(|i| GetBlockByNumber { block_number: i })
                                .collect::<Vec<_>>();
                            async move { client.send_batch(&batch).await }
                        })
                    })
                    .collect::<Vec<_>>();

                let group = futures::future::join_all(group).await;

                for batch in group {
                    let mut batch = match batch {
                        Ok(batch) => batch,
                        Err(e) => {
                            eprintln!("failed batch block req: {:#?}", e);
                            continue;
                        }
                    };
                    for block in batch.iter_mut() {
                        tx_writer.send(mem::take(&mut block.transactions));
                    }
                    block_writer.send(batch);
                }
                println!(
                    "TX/BLOCK WRITER: processed {} blocks in {} ms",
                    STEP * BATCH_SIZE,
                    start.elapsed().as_millis()
                )
            }
        }
    });

    let block_range = 10_000_000..14_000_000;

    let log_job = tokio::spawn({
        let client = client.clone();
        async move {
            const BATCH_SIZE: usize = 5;
            const STEP: usize = 100;

            for block_num in block_range.step_by(STEP * BATCH_SIZE) {
                let start = Instant::now();
                let group = (0..STEP)
                    .map(|step| {
                        let start = block_num + step * BATCH_SIZE;
                        let end = start + BATCH_SIZE;

                        let client = client.clone();
                        async move {
                            client
                                .send(GetLogs {
                                    from_block: start,
                                    to_block: end,
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
                            eprintln!("failed batch log req: {:#?}", e);
                            continue;
                        }
                    };

                    log_writer.send(batch);
                }
                println!(
                    "LOG WRITER: processed {} blocks in {} ms",
                    STEP * BATCH_SIZE,
                    start.elapsed().as_millis()
                )
            }
        }
    });

    block_tx_job.await.unwrap();
    log_job.await.unwrap();
}
