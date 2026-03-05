use std::fs::File;
use std::io::BufReader;

use serde_yaml::from_reader;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct RadioNetwork {
    pub name: String,
    pub start_freq: u64,
    pub end_freq: u64,
    pub step: u64,
}

impl RadioNetwork {
    const ERROR_NO_NAME: &'static str = "No name defined.";
    const ERROR_NO_START_FREQ: &'static str = "No start_freq defined.";
    const ERROR_NO_END_FREQ: &'static str = "No end_freq defined.";
    const ERROR_NO_STEP: &'static str = "No step defined.";
    const ERROR_START_FREQ_GREATER_THAN_END_FREQ: &'static str = "Start_freq must be less than end_freq.";
    const ERROR_STEP_NOT_FACTOR: &'static str = "Step must be a factor of the difference between start_freq and end_freq.";

    pub fn validate_values(&self) -> Result<(), anyhow::Error> {
        if self.name.is_empty() {
            return Err(anyhow::anyhow!(Self::ERROR_NO_NAME));
        }

        if self.start_freq == 0 {
            return Err(anyhow::anyhow!(Self::ERROR_NO_START_FREQ));
        }

        if self.end_freq == 0 {
            return Err(anyhow::anyhow!(Self::ERROR_NO_END_FREQ));
        }

        if self.step == 0 {
            return Err(anyhow::anyhow!(Self::ERROR_NO_STEP));
        }

        if self.start_freq >= self.end_freq {
            return Err(anyhow::anyhow!(Self::ERROR_START_FREQ_GREATER_THAN_END_FREQ));
        }

        if (self.end_freq - self.start_freq) % self.step != 0 {
            return Err(anyhow::anyhow!(Self::ERROR_STEP_NOT_FACTOR));
        }

        Ok(())
    }

}


#[derive(Clone, Debug, Deserialize)]
pub struct RadioNetworks {
    pub networks: Vec<RadioNetwork>,
    #[serde(skip)]
    pub scan_frequencies: Vec<u32>,
}

impl RadioNetworks {

    const ERROR_NO_NETWORKS: &'static str = "No networks defined.";

    pub fn empty() -> Self {
        Self {
            networks: Vec::new(),
            scan_frequencies: Vec::new(),
        }
    }

    pub async fn from_yaml(file_path: &str) -> Result<RadioNetworks, anyhow::Error> {
        let mut networks = Self::read_from_yaml(file_path).await?;
        networks.validate_values().await?;
        networks.build_scan_frequencies();
        
        // Return both the networks object and the OK
        Ok(networks)
    }

    pub async fn read_from_yaml(file_path: &str) -> Result<RadioNetworks, anyhow::Error> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let networks = from_reader(reader)?;
        Ok(networks)
    }

    pub fn network_name_from_freq(&self, freq: u64) -> Option<String> {
        for network in &self.networks {
            if freq >= network.start_freq && freq <= network.end_freq {
                return Some(network.name.clone());
            }
        }
        None
    }

    pub fn build_scan_frequencies(&mut self) {
        self.scan_frequencies.clear();
        for network in &self.networks {
            for freq in (network.start_freq..network.end_freq).step_by(network.step as usize) {
                self.scan_frequencies.push(freq as u32);
            }
        }
    }

    pub fn scan_frequencies(&self) -> &[u32] {
        &self.scan_frequencies
    }

    pub async fn validate_values(&self) -> Result<(), anyhow::Error> {
        if self.networks.is_empty() {
            return Err(anyhow::anyhow!(Self::ERROR_NO_NETWORKS));
        }

        self.validate_network_values().await?;

        Ok(())
    }

    pub async fn validate_network_values(&self) -> Result<(), anyhow::Error> {
        for network in &self.networks {
            network.validate_values()?;
        }

        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_networks_from_yaml() {
        let networks = get_testing_data().await;
        assert_eq!(networks.networks.len(), 2);
    }

    #[tokio::test]
    async fn test_get_network_name_from_freq() {
        let networks = get_testing_data().await;
        assert_eq!(networks.network_name_from_freq(25000001).unwrap(), "pubnet");
        assert_eq!(networks.network_name_from_freq(823000000).unwrap(), "scavnet");
        assert_eq!(networks.network_name_from_freq(10000), None);
    }

    #[tokio::test]
    async fn test_build_scan_frequencies() {
        let networks = get_testing_data().await;
        assert_eq!(networks.scan_frequencies.len(), 55300000);
    }

    #[tokio::test]
    async fn test_no_name() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].name = String::new();
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_NO_NAME);
    }

    #[tokio::test]
    async fn test_no_start_freq() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].start_freq = 0;
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_NO_START_FREQ);
    }

    #[tokio::test]
    async fn test_no_end_freq() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].end_freq = 0;
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_NO_END_FREQ);
    }

    #[tokio::test]
    async fn test_no_step() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].step = 0;
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_NO_STEP);
    }

    #[tokio::test]
    async fn test_invalid_network_freqs() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].start_freq = 512000001;
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_START_FREQ_GREATER_THAN_END_FREQ);
    }

    #[tokio::test]
    async fn test_invalid_network_step() {
        let mut networks = get_raw_testing_data().await;
        networks.networks[0].end_freq = 511999999;
        let result = networks.validate_values().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), RadioNetwork::ERROR_STEP_NOT_FACTOR);
    }

    fn test_data_path() -> String {
        format!("{}/test/data/networks.yaml", env!("CARGO_MANIFEST_DIR"))
    }

    async fn get_testing_data() -> RadioNetworks {
        RadioNetworks::from_yaml(&test_data_path()).await.unwrap()
    }

    async fn get_raw_testing_data() -> RadioNetworks {
        RadioNetworks::read_from_yaml(&test_data_path()).await.unwrap()
    }
}

pub async fn init_networks(network_library_path: String) -> Result<RadioNetworks, anyhow::Error> {
    let radio_networks_result = RadioNetworks::from_yaml(network_library_path.as_str()).await?;
    Ok(radio_networks_result)
}
