#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sample {
    Distance(f64),
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lock,
    Unlock,
}

pub struct Monitor {
    locked: bool,
    far_streak: u32,
    away_streak: u32,
    max_missed: u32,
    max_distance_cm: f64,
}

impl Monitor {
    pub fn new(max_missed: u32, max_distance_cm: f64) -> Self {
        Monitor {
            locked: true,
            far_streak: 0,
            away_streak: 0,
            max_missed,
            max_distance_cm,
        }
    }

    pub fn update(&mut self, sample: &Sample) -> Option<Action> {
        match sample {
            Sample::Away => {
                self.away_streak += 1;
                self.far_streak = 0;
                if self.away_streak >= self.max_missed {
                    self.transition(true)
                } else {
                    None
                }
            }
            Sample::Distance(d) => {
                self.away_streak = 0;
                if d * 100.0 > self.max_distance_cm {
                    self.far_streak += 1;
                    if self.far_streak >= self.max_missed {
                        self.transition(true)
                    } else {
                        None
                    }
                } else {
                    self.far_streak = 0;
                    self.transition(false)
                }
            }
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    fn transition(&mut self, locked: bool) -> Option<Action> {
        if self.locked == locked {
            None
        } else {
            self.locked = locked;
            Some(if locked { Action::Lock } else { Action::Unlock })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_locked() {
        let m = Monitor::new(3, 90.0);
        assert!(m.is_locked());
    }

    #[test]
    fn test_near_unlocks() {
        let mut m = Monitor::new(3, 90.0);
        let action = m.update(&Sample::Distance(0.5)); // 50 cm < 90 cm
        assert_eq!(action, Some(Action::Unlock));
        assert!(!m.is_locked());
    }

    #[test]
    fn test_far_streak_locks_after_threshold() {
        let mut m = Monitor::new(3, 90.0);
        m.update(&Sample::Distance(0.5)); // unlock first
        assert!(!m.is_locked());

        assert_eq!(m.update(&Sample::Distance(2.0)), None); // far 1/3
        assert_eq!(m.update(&Sample::Distance(2.0)), None); // far 2/3
        assert_eq!(m.update(&Sample::Distance(2.0)), Some(Action::Lock)); // far 3/3
        assert!(m.is_locked());
    }

    #[test]
    fn test_away_streak_locks_after_threshold() {
        let mut m = Monitor::new(2, 90.0);
        m.update(&Sample::Distance(0.5)); // unlock
        assert_eq!(m.update(&Sample::Away), None); // away 1/2
        assert_eq!(m.update(&Sample::Away), Some(Action::Lock)); // away 2/2
    }

    #[test]
    fn test_near_resets_far_streak() {
        let mut m = Monitor::new(3, 90.0);
        m.update(&Sample::Distance(0.5)); // unlock
        m.update(&Sample::Distance(2.0)); // far 1
        m.update(&Sample::Distance(2.0)); // far 2
        m.update(&Sample::Distance(0.5)); // near, resets far streak, no-op (already unlocked)
        assert_eq!(m.update(&Sample::Distance(2.0)), None); // far 1 again
        assert!(!m.is_locked());
    }

    #[test]
    fn test_away_resets_on_distance() {
        let mut m = Monitor::new(3, 90.0);
        m.update(&Sample::Distance(0.5)); // unlock
        m.update(&Sample::Away); // away 1
        m.update(&Sample::Distance(0.5)); // resets away streak
        assert_eq!(m.update(&Sample::Away), None); // away 1 again
    }

    #[test]
    fn test_no_duplicate_lock() {
        let mut m = Monitor::new(1, 90.0);
        // starts locked, away should not re-lock
        assert_eq!(m.update(&Sample::Away), None);
        assert!(m.is_locked());
    }

    #[test]
    fn test_no_duplicate_unlock() {
        let mut m = Monitor::new(3, 90.0);
        assert_eq!(m.update(&Sample::Distance(0.5)), Some(Action::Unlock));
        assert_eq!(m.update(&Sample::Distance(0.5)), None); // already unlocked
    }

    #[test]
    fn test_boundary_distance() {
        let mut m = Monitor::new(3, 90.0);
        m.update(&Sample::Distance(0.5)); // unlock
                                          // exactly 90 cm is NOT far (uses > comparison)
        assert_eq!(m.update(&Sample::Distance(0.9)), None);
        assert!(!m.is_locked());
        // just over 90 cm is far
        assert_eq!(m.update(&Sample::Distance(0.9001)), None); // far 1/3
    }
}
