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

    pub fn run<F, Fut>(&self, f: F) -> JoinHandle<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let permit = self.semaphore.clone().acquire_owned();
        tokio::task::spawn(async move {
            let _permit = permit.await.unwrap();
            f().await;
        })
    }
}
