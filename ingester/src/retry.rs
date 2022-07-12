use crate::config::RetryConfig;
use crate::Error;
use std::future::Future;

pub struct Retry {
    num_tries: Option<usize>,
    secs_between_tries: u64,
}

impl Retry {
    pub fn new(cfg: RetryConfig) -> Self {
        Self {
            num_tries: cfg.num_tries,
            secs_between_tries: cfg.secs_between_tries,
        }
    }

    pub async fn retry<F, T, Fut>(&self, mut action: F) -> Result<T, Vec<Error>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        if let Some(num_tries) = self.num_tries {
            let mut errs = Vec::new();
            for _ in 0..num_tries {
                match action().await {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        log::error!("failed to run operation: {:#?}, retrying", e);
                        tokio::time::sleep(std::time::Duration::from_secs(self.secs_between_tries))
                            .await;
                        errs.push(e);
                    }
                }
            }
            Err(errs)
        } else {
            loop {
                match action().await {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        log::error!("failed to run operation: {:#?}, retrying", e);
                        tokio::time::sleep(std::time::Duration::from_secs(self.secs_between_tries))
                            .await;
                    }
                }
            }
        }
    }
}
