use rustc_hash::FxHashMap;
/// MacJet MCP — Async TTL Cache for collector results.
/// Prevents agent burst calls from pegging the CPU with repeated psutil scans.
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

pub struct AsyncTTLCache {
    ttl: Duration,
    cache: RwLock<FxHashMap<String, (Instant, String)>>,
    locks: RwLock<FxHashMap<String, Arc<Mutex<()>>>>,
}

impl AsyncTTLCache {
    pub fn new(ttl_secs: f64) -> Self {
        Self {
            ttl: Duration::from_secs_f64(ttl_secs),
            cache: RwLock::new(FxHashMap::default()),
            locks: RwLock::new(FxHashMap::default()),
        }
    }

    /// Recursively async-safe TTL retrieval matching the python implementation
    pub async fn get<F, Fut>(&self, key: &str, factory: F) -> String
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = String>,
    {
        // Fast path: cache hit
        let now = Instant::now();
        {
            let cache_read = self.cache.read().await;
            if let Some((ts, value)) = cache_read.get(key) {
                if now.duration_since(*ts) < self.ttl {
                    return value.clone();
                }
            }
        }

        // Lock retrieval to avoid thundering herd on data compilation
        let lock = {
            let mut locks_write = self.locks.write().await;
            locks_write
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = lock.lock().await;

        // Double check cache before computation
        let now = Instant::now();
        {
            let cache_read = self.cache.read().await;
            if let Some((ts, value)) = cache_read.get(key) {
                if now.duration_since(*ts) < self.ttl {
                    return value.clone();
                }
            }
        }

        // Recompute the snapshot entirely
        let value = factory().await;

        let mut cache_write = self.cache.write().await;
        cache_write.insert(key.to_string(), (Instant::now(), value.clone()));

        value
    }

    pub async fn invalidate(&self, key: Option<&str>) {
        let mut cache_write = self.cache.write().await;
        if let Some(k) = key {
            cache_write.remove(k);
        } else {
            cache_write.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_cache_hit_returns_same_value() {
        let cache = AsyncTTLCache::new(10.0);
        let count = Arc::new(AtomicUsize::new(0));

        let _first = cache
            .get("key", || async {
                count.fetch_add(1, Ordering::SeqCst);
                "result".to_string()
            })
            .await;

        let _second = cache
            .get("key", || async {
                count.fetch_add(1, Ordering::SeqCst);
                "result2".to_string()
            })
            .await;

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
