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
