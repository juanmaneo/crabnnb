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
