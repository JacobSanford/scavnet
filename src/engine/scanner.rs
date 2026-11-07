//! Sweeps scan indexes at a fixed rate, independent of how often the screen redraws.

pub struct Scanner {
    pos: u64,
    total: u64,
    per_second: f64,
    carry: f64,
}

impl Scanner {
    pub fn new(total: u64, per_second: u64) -> Self {
        assert!(total > 0, "frequency plan must not be empty");
        Self {
            pos: 0,
            total,
            per_second: per_second as f64,
            carry: 0.0,
        }
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Advances by `dt` seconds. If `target` lies in the swept span `(pos, pos + n]`,
    /// stops on it and returns true. A long `dt` sweeps at most one full cycle.
    pub fn sweep(&mut self, dt: f64, target: Option<u64>) -> bool {
        let exact = self.per_second * dt + self.carry;
        // The epsilon keeps accumulated float error from losing a step (2.4999999 -> 2).
        let steps = (exact + 1e-9).floor();
        self.carry = (exact - steps).max(0.0);
        let steps = steps as u64;
        if steps == 0 {
            return false;
        }
        if let Some(target) = target {
            let distance = (target % self.total + self.total - self.pos - 1) % self.total;
            if distance < steps.min(self.total) {
                self.pos = target % self.total;
                self.carry = 0.0;
                return true;
            }
        }
        self.pos = (self.pos + steps % self.total) % self.total;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_at_rate_with_fractional_carry() {
        let mut s = Scanner::new(1000, 25);
        for _ in 0..10 {
            s.sweep(0.1, None);
        }
        assert_eq!(s.position(), 25);
    }

    #[test]
    fn wraps_around() {
        let mut s = Scanner::new(10, 100);
        s.sweep(0.13, None);
        assert_eq!(s.position(), 3);
    }

    #[test]
    fn hits_target_inside_swept_span_only() {
        let mut s = Scanner::new(100, 100);
        assert!(!s.sweep(0.05, Some(50)), "swept 1..=5");
        assert_eq!(s.position(), 5);
        assert!(s.sweep(0.5, Some(50)), "swept 6..=55");
        assert_eq!(s.position(), 50);
        assert!(!s.sweep(0.01, Some(50)), "current position is not re-detected");
    }

    #[test]
    fn hits_across_the_wrap() {
        let mut s = Scanner::new(100, 100);
        s.sweep(0.95, None);
        assert!(s.sweep(0.1, Some(2)));
        assert_eq!(s.position(), 2);
    }

    #[test]
    fn huge_dt_is_one_cycle_and_still_hits() {
        let mut s = Scanner::new(100, 1_000_000);
        assert!(s.sweep(60.0, Some(0)));
    }
}
