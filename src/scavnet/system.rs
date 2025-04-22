use super::super::SETTINGS;

pub struct System {
    log_events: Vec<String>,
}

impl System {
    pub fn new() -> Self {
        System {
            log_events: Vec::new(),
        }
    }

    pub fn debug_log(&mut self, event: String) {
        let debug = SETTINGS.lock().get_bool("debug").unwrap();
        if debug == true {
            self.log(event);
        }
    }

    pub fn log(&mut self, event: String) {
        let event = format!(
            "{}: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            event
        );
        self.log_events.push(event);
    }

    pub fn _get_logs(&self) -> &Vec<String> {
        &self.log_events
    }

    pub fn get_last_x_logs(&self, x: usize) -> Vec<String> {
        let len = self.log_events.len();
        if x >= len {
            self.log_events.clone()
        } else {
            self.log_events[len - x..].to_vec()
        }
    }
}
