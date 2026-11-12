use std::time::Duration;

pub trait Clock {
    fn sleep(&mut self, d: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    struct MockClock {
        slept: Arc<Mutex<Vec<Duration>>>,
    }

    impl Clock for MockClock {
        fn sleep(&mut self, d: Duration) {
            self.slept.lock().unwrap().push(d);
        }
    }

    #[test]
    fn test_system_clock_sleeps() {
        let mut clock = SystemClock;
        let start = Instant::now();
        clock.sleep(Duration::from_millis(10));
        assert!(start.elapsed() >= Duration::from_millis(10));
    }

    #[test]
    fn test_mock_clock_records() {
        let slept = Arc::new(Mutex::new(Vec::new()));
        let mut clock = MockClock {
            slept: Arc::clone(&slept),
        };
        clock.sleep(Duration::from_millis(5));
        clock.sleep(Duration::from_secs(1));
        let recorded = slept.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], Duration::from_millis(5));
        assert_eq!(recorded[1], Duration::from_secs(1));
    }
}
