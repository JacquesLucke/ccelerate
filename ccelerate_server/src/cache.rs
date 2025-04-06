use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;

pub struct Cache<Key: Eq + std::hash::Hash + Clone, Value: Send + Sync + 'static> {
    map: Mutex<std::collections::HashMap<Key, Arc<CacheValue<Value>>>>,
}

struct CacheValue<Value> {
    value: tokio::sync::watch::Receiver<Option<Arc<Value>>>,
}

impl<Key: Eq + std::hash::Hash + Clone, Value: Send + Sync + 'static> Cache<Key, Value> {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn get<F, Fut>(&self, key: &Key, f: F) -> Result<Arc<Value>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Value>,
    {
        {
            let (sender, cache_value) = {
                let mut map = self.map.lock();
                if let Some(value) = map.get(key) {
                    let value = value.clone();
                    (None, value)
                } else {
                    let (sender, receiver) = tokio::sync::watch::channel(None);
                    let cache_value = Arc::new(CacheValue { value: receiver });
                    map.insert(key.clone(), cache_value.clone());
                    (Some(sender), cache_value)
                }
            };
            match sender {
                Some(sender) => {
                    let value = f().await;
                    let value = Arc::new(value);
                    sender.send(Some(value.clone()))?;
                    Ok(value)
                }
                None => {
                    let mut receiver = cache_value.value.clone();
                    let value = receiver.wait_for(|v| v.is_some()).await?;
                    Ok(value.clone().unwrap())
                }
            }
        }
    }

    pub fn get_entries(&self) -> Vec<CacheEntry<Key, Value>> {
        self.map
            .lock()
            .iter()
            .filter_map(|(key, value)| {
                let value = value.value.borrow();
                (*value).clone().map(|value| CacheEntry {
                    key: key.clone(),
                    value: value.clone(),
                })
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry<Key, Value> {
    pub key: Key,
    pub value: Arc<Value>,
}
