#![deny(clippy::unwrap_used)]

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tokio::sync::watch;

pub struct ComputeCache<
    Key: Eq + std::hash::Hash + Clone,
    KeyTime: Eq + std::hash::Hash + Ord + Clone,
    Value: Send + Sync + Clone + 'static,
> {
    map: Mutex<HashMap<Key, ValuesForKey<KeyTime, Value>>>,
}

struct ValuesForKey<KeyTime, Value> {
    values_by_key: HashMap<KeyTime, Arc<CacheValue<Value>>>,
}

struct CacheValue<Value> {
    value: watch::Receiver<Option<Value>>,
}

impl<
    Key: Eq + std::hash::Hash + Clone,
    KeyTime: Eq + std::hash::Hash + Ord + Clone,
    Value: Send + Sync + Clone + 'static,
> ComputeCache<Key, KeyTime, Value>
{
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get<F, Fut>(&self, key: &Key, time: &KeyTime, f: F) -> Value
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Value>,
    {
        let (sender, cache_value) = {
            let mut map = self.map.lock();
            if let Some(values_for_key) = map.get_mut(key) {
                if let Some(cache_value) = values_for_key.values_by_key.get_mut(time) {
                    (None, cache_value.clone())
                } else {
                    // Remove older keys. If computations for those are still in flight, they will finish normally.
                    values_for_key.values_by_key.retain(|k, _| k > time);
                    let (sender, receiver) = watch::channel(None);
                    let cache_value = Arc::new(CacheValue { value: receiver });
                    values_for_key
                        .values_by_key
                        .insert(time.clone(), cache_value.clone());
                    (Some(sender), cache_value)
                }
            } else {
                let (sender, receiver) = watch::channel(None);
                let cache_value = Arc::new(CacheValue { value: receiver });
                let mut values_for_key = ValuesForKey {
                    values_by_key: HashMap::new(),
                };
                values_for_key
                    .values_by_key
                    .insert(time.clone(), cache_value.clone());
                map.insert(key.clone(), values_for_key);
                (Some(sender), cache_value)
            }
        };
        match sender {
            Some(sender) => {
                let value = f().await;
                sender.send(Some(value.clone())).ok();
                value
            }
            None => {
                let mut receiver = cache_value.value.clone();
                let value = receiver
                    .wait_for(|v| v.is_some())
                    .await
                    .expect("the channel is never closed");
                value.clone().expect("has to be available")
            }
        }
    }

    pub fn _for_each_latest<F>(&self, mut f: F)
    where
        F: FnMut(&Key, &KeyTime, &Value),
    {
        for (key, values_for_key) in self.map.lock().iter() {
            if let Some(max_time) = values_for_key.values_by_key.keys().max() {
                if let Some(value) = values_for_key.values_by_key.get(max_time) {
                    if let Some(value) = value.value.borrow().as_ref() {
                        f(key, max_time, value);
                    }
                }
            }
        }
    }
}
