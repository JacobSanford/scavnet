//! A bounded, timestamped event log for the SYSTEM panel.

use std::collections::VecDeque;

const CAPACITY: usize = 500;

pub struct Log {
    lines: VecDeque<String>,
    debug: bool,
}

impl Log {
    pub fn new(debug: bool) -> Self {
        Self {
            lines: VecDeque::with_capacity(CAPACITY),
            debug,
        }
    }

    pub fn info(&mut self, message: impl AsRef<str>) {
        if self.lines.len() == CAPACITY {
            self.lines.pop_front();
        }
        self.lines.push_back(format!(
            "{} {}",
            chrono::Local::now().format("%H:%M:%S"),
            message.as_ref()
        ));
    }

    /// Only recorded when debug is on. The closure is not run otherwise.
    pub fn debug(&mut self, message: impl FnOnce() -> String) {
        if self.debug {
            self.info(message());
        }
    }

    /// The newest `n` lines, oldest first.
    pub fn tail(&self, n: usize) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .skip(self.lines.len().saturating_sub(n))
            .map(String::as_str)
    }

    #[cfg(test)]
    pub fn contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|l| l.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_tail_ordered() {
        let mut log = Log::new(false);
        for i in 0..CAPACITY + 10 {
            log.info(format!("line {i}"));
        }
        assert_eq!(log.lines.len(), CAPACITY);
        let tail: Vec<_> = log.tail(2).collect();
        assert!(tail[0].ends_with(&format!("line {}", CAPACITY + 8)));
        assert!(tail[1].ends_with(&format!("line {}", CAPACITY + 9)));
        assert!(!log.contains("line 0 "));
    }

    #[test]
    fn debug_lines_respect_flag() {
        let mut off = Log::new(false);
        off.debug(|| unreachable!("not evaluated when debug is off"));
        assert_eq!(off.tail(10).count(), 0);
        let mut on = Log::new(true);
        on.debug(|| "x".into());
        assert_eq!(on.tail(10).count(), 1);
    }
}
