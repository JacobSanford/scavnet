use std::error::Error;
use std::fs::{self, File};
use std::path::Path;
use std::io::{self, BufWriter, BufReader};

use hound;
use rayon::prelude::*;
use rmp_serde::{encode::write, decode::from_read};
use serde::{Deserialize, Serialize};
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;

const HANN_WINDOW_SIZE: usize = 2048;
const FFT_MAX_VALUE: f32 = 120.0;
const FFT_NORMALIZATION_FACTOR: f32 = 2.5;

#[derive(Serialize, Deserialize)]
struct FFTData(Vec<Vec<f32>>);

pub fn write_fft_all_wav_files_in_dir(dir: &Path) {
    let mut files = fs::read_dir(dir).unwrap();
    while let Some(file) = files.next() {
        let file = file.unwrap();
        let path = file.path();
        if path.is_file() {
            if path.extension().unwrap() == "wav" {
                let path_str = path.to_str().unwrap();
                if let Err(e) = write_fft_data(&path_str) {
                    eprintln!("Error writing FFT data: {}", e);
                }
            }
        } else if path.is_dir() {
            write_fft_all_wav_files_in_dir(&path);
        }
    }
}

pub fn write_fft_data(file_path: &str) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let normalized_data = fft_from_path(file_path)?;
    let fft_data_filepath = format!("{}.fft", file_path);

    // Scale the data from 0 to 100
    let scaled_data: Vec<Vec<f32>> = normalized_data.iter()
        .map(|v| {
            let max = v.iter().copied().reduce(f32::max).unwrap_or(f32::MIN);
            v.iter().map(|x| x / max * 120.0).collect()
        })
        .collect();

    let file = File::create(&fft_data_filepath)?;
    let mut writer = BufWriter::new(file);
    write(&mut writer, &FFTData(scaled_data.clone()))?;

    Ok(normalized_data)
}

pub fn read_fft_data(file_path: &str) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let fft_data_filepath = format!("{}.fft", file_path);
    // If the file doesn't exist, return an error
    if !std::path::Path::new(&fft_data_filepath).exists() {
        return Err(Box::from("FFT data file does not exist"));
    }
    let file = File::open(&fft_data_filepath)?;
    let reader = BufReader::new(file);
    let fft_data: FFTData = from_read(reader)?;
    Ok(fft_data.0)
}

pub fn fft_normalized_from_path(file_path: &str) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let mut reader: hound::WavReader<io::BufReader<File>> = hound::WavReader::open(file_path)?;
    fft_normalized(&mut reader)
}

pub fn fft_cached_normalized(file_path: &str) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let fft_data = read_fft_data(file_path);
    if fft_data.is_ok() {
        return Ok(fft_data.unwrap());
    }
    let fft_data = fft_normalized_from_path(file_path)?;
    Ok(fft_data)
}

pub fn fft_normalized(reader: &mut hound::WavReader<io::BufReader<File>>) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let fft_data = fft(reader)?;
    let normalized_data = normalize_fft_data(fft_data)?;
    Ok(normalized_data)
}

pub fn normalize_fft_data(fft_data: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>, Box<dyn Error>>  {
    let mut normalized_data: Vec<Vec<f32>> = Vec::new();
    let maximum_value = FFT_MAX_VALUE;
    let current_max = fft_data.iter()
        .flat_map(|v| v.iter())
        .copied()
        .reduce(f32::max)
        .unwrap_or(f32::MIN);
    
    // Normalize the data
    let normalization_factor = maximum_value / current_max * FFT_NORMALIZATION_FACTOR;
    for item in fft_data {
        let normalized_item: Vec<f32> = item.par_iter()
            .map(|&x| x * normalization_factor)
            .collect();
        normalized_data.push(normalized_item);
    }

    Ok(normalized_data)
}

pub fn fft_from_path(file_path: &str) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let mut reader: hound::WavReader<io::BufReader<File>> = hound::WavReader::open(file_path)?;
    fft(&mut reader)
}

pub fn fft(reader: &mut hound::WavReader<io::BufReader<File>>) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let wav_sample_rate = reader.spec().sample_rate;
    let samples: Vec<f32> = reader.samples::<i16>()
        .map(|s| s.unwrap() as f32)
        .collect();

    let window_size = HANN_WINDOW_SIZE;
    let len_samples = samples.len();

    // Use rayon to parallelize the computation
    let fft_data: Vec<Vec<f32>> = (0..len_samples)
        .step_by(window_size)
        .collect::<Vec<_>>()
        .into_par_iter()
        .filter_map(|sample_index_start| {
            let sample_index_end = sample_index_start + window_size;
            if sample_index_end > len_samples {
                return None;
            }
            let sample_set: Vec<f32> = samples[sample_index_start..sample_index_end].to_vec();

            let hann_window = hann_window(&sample_set);
            let spectrum_hann_window = samples_fft_to_spectrum(
                &hann_window,
                wav_sample_rate,
                FrequencyLimit::All,
                Some(&divide_by_N_sqrt),
            ).ok()?;

            // Convert the spectrum to a Vec<f32>
            let fft_data_item: Vec<f32> = spectrum_hann_window.data()
                .iter()
                .map(|(_fr, fr_val)| fr_val.val())
                .collect();

            Some(fft_data_item)
        })
        .collect();

    Ok(fft_data)
}