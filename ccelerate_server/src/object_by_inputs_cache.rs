#![deny(clippy::unwrap_used)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;

use crate::compute_cache::ComputeCache;

pub struct ObjectByInputsCache {
    cache: ComputeCache<Vec<PathBuf>, chrono::DateTime<chrono::FixedOffset>, Arc<Result<PathBuf>>>,
}

impl ObjectByInputsCache {
    pub fn new() -> Self {
        Self {
            cache: ComputeCache::new(),
        }
    }

    pub async fn get<F, Fut>(
        &self,
        inputs: &[impl AsRef<Path>],
        time: chrono::DateTime<chrono::FixedOffset>,
        build_object: F,
    ) -> Arc<Result<PathBuf>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<PathBuf>>,
    {
        self.cache
            .get(
                &inputs.iter().map(|p| p.as_ref().to_owned()).collect(),
                &time,
                async || Arc::new(build_object().await),
            )
            .await
    }
}
