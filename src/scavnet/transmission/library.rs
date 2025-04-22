use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rand::Rng;
use serde_yaml::from_reader;
use serde::Deserialize;

use crate::scavnet::fft::write_fft_all_wav_files_in_dir;
use crate::scavnet::settings::get_data_dir;

#[derive(Debug, Deserialize)]
struct Set {
    name: String,
    weight: f64,
    #[serde(default)]
    sets: Option<Vec<Set>>,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RootSet {
    sets: Vec<Set>, // Top-level sets
}

#[derive(Debug, Clone)]
pub struct TransmissionSetNode {
    value: String,
    pub data: Option<String>,
    branches: HashMap<String, (f64, Box<TransmissionSetNode>)>,
}

impl TransmissionSetNode {
    fn new(value: String, data: Option<String>) -> Self {
        TransmissionSetNode {
            value,
            data,
            branches: HashMap::new(),
        }
    }

    fn add_branch(&mut self, label: String, weight: f64, child: TransmissionSetNode) {
        self.branches.insert(label, (weight, Box::new(child)));
    }

    pub fn traverse(&self, rng: &mut impl Rng) -> &Self {
        let total_weight: f64 = self.branches.values().map(|(weight, _)| weight).sum();
        if total_weight == 0.0 {
            return self;
        }

        let mut cumulative_weight = 0.0;
        let random_value: f64 = rng.gen::<f64>() * total_weight;

        for (_label, (weight, child)) in &self.branches {
            cumulative_weight += weight;
            if random_value < cumulative_weight {
                return child.traverse(rng);
            }
        }

        self
    }

    fn build_tree(set: &Set, base_dir: String) -> TransmissionSetNode {
        let sub_dir = set.data.clone();
        let full_path = format!("{}/transmissions/{}", base_dir, sub_dir.unwrap_or_default());

        let mut node = TransmissionSetNode::new(set.name.clone(), Some(full_path.clone()));

        if let Some(child_sets) = &set.sets {
            for child_set in child_sets {
                let child_node = Self::build_tree(child_set, base_dir.clone());
                node.add_branch(child_set.name.clone(), child_set.weight, child_node);
            }
        }

        node
    }

    pub fn load_tree_from_yaml_file(file_path: &Path) -> Result<TransmissionSetNode, Box<dyn std::error::Error>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let root_set: RootSet = from_reader(reader)?;
        let base_dir = get_data_dir();

        let mut root_node = TransmissionSetNode::new("root".to_string(), None);
        for set in root_set.sets {
            let child_node = Self::build_tree(&set, base_dir.clone());
            root_node.add_branch(set.name.clone(), set.weight, child_node);
        }

        Ok(root_node)
    }

}

#[derive(Clone)]
pub struct TransmissionLibrary {
    library: TransmissionSetNode,
}

impl TransmissionLibrary {
    pub fn empty() -> Self {
        Self {
            library: TransmissionSetNode::new("root".to_string(), None),
        }
    }

    pub fn new(library: TransmissionSetNode) -> Self {
        Self {
            library,
        }
    }

    pub fn choose(&self, rng: &mut impl Rng) -> &TransmissionSetNode {
        self.library.traverse(rng)
    }

    pub fn build(file_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let library = TransmissionSetNode::load_tree_from_yaml_file(file_path)?;
        Ok(Self::new(library))
    }
}

pub async fn build_transmission_library(file_path: &str) -> Result<TransmissionLibrary, Box<dyn Error>> {
    let transmission_path = std::path::Path::new(&file_path);
    precompute_library_ffts();
    TransmissionLibrary::build(transmission_path)
}

fn precompute_library_ffts() {
    let data_dir = get_data_dir();
    let transmission_path_str = format!("{}/transmissions", data_dir);
    let transmission_path = std::path::Path::new(&transmission_path_str);
    write_fft_all_wav_files_in_dir(transmission_path);
}
