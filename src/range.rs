use rand::Rng;
use serde::Deserialize;

/// An inclusive range of seconds, sampled uniformly. `min == max` is a fixed value.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

impl Range {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.min.is_finite() || !self.max.is_finite() {
            return Err(format!("min and max must be finite numbers (got {} .. {})", self.min, self.max));
        }
        if self.min < 0.0 {
            return Err(format!("min must not be negative (got {})", self.min));
        }
        if self.min > self.max {
            return Err(format!("min must not exceed max (got {} .. {})", self.min, self.max));
        }
        Ok(())
    }

    pub fn sample(&self, rng: &mut impl Rng) -> f32 {
        if self.min >= self.max {
            self.min
        } else {
            rng.random_range(self.min..=self.max)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn equal_bounds_return_fixed_value() {
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(Range::new(2.5, 2.5).sample(&mut rng), 2.5);
    }

    #[test]
    fn samples_stay_in_bounds() {
        let mut rng = StdRng::seed_from_u64(1);
        let r = Range::new(1.0, 3.0);
        for _ in 0..1000 {
            let v = r.sample(&mut rng);
            assert!((1.0..=3.0).contains(&v));
        }
    }

    #[test]
    fn validation_rejects_bad_ranges() {
        assert!(Range::new(3.0, 1.0).validate().is_err());
        assert!(Range::new(-1.0, 1.0).validate().is_err());
        assert!(Range::new(f32::NAN, 1.0).validate().is_err());
        assert!(Range::new(0.0, 0.0).validate().is_ok());
    }
}
