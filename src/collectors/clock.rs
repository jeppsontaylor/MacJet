/// MacJet — Time Control Trait
///
/// Provides a deterministic clock for tests and a real system clock for production.
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Returns the current time in seconds since the UNIX epoch.
    fn now(&self) -> f64;
}

#[derive(Clone, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A controllable clock for deterministic testing.
    /// Equivalent to `FakeClock` in the Python test suite (`conftest.py`).
    #[derive(Clone)]
    pub struct FakeClock {
        time: Arc<Mutex<f64>>,
    }

    impl FakeClock {
        pub fn new(start: f64) -> Self {
            Self {
                time: Arc::new(Mutex::new(start)),
            }
        }

        pub fn advance(&self, seconds: f64) {
            let mut t = self.time.lock().unwrap();
            *t += seconds;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> f64 {
            *self.time.lock().unwrap()
        }
    }
}
