#![deny(clippy::unwrap_used)]

use std::sync::Arc;

use tokio::task::JoinHandle;

pub struct ParallelPool {
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl ParallelPool {
    pub fn new(num: usize) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(num)),
        }
    }

    pub fn run_separate_thread<F, Fut, Out>(&self, f: F) -> JoinHandle<Out>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Out> + Send + 'static,
        Out: Send + 'static,
    {
        let permit = self.semaphore.clone().acquire_owned();
        tokio::task::spawn(async move {
            let _permit = permit.await.expect("should be valid");
            f().await
        })
    }

    pub async fn run_same_thread<F, Fut, Out>(&self, f: F) -> Out
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Out>,
    {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("should always succeed eventually");
        f().await
    }
}
