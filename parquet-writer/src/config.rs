use eth_archive_core::config::{DbConfig, IngestConfig, RetryConfig};
use parquet_writer::ParquetConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub block: ParquetConfig,
    pub transaction: ParquetConfig,
    pub log: ParquetConfig,
    pub ingest: IngestConfig,
    pub retry: RetryConfig,
    pub db: DbConfig,
    pub block_overlap_size: usize,
    pub delete_blocks_chunk_size: usize,
}
