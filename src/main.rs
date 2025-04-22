use std::{
    io,
    ops::Add,
    thread,
    time::Duration,
};

use config::Config;
use cpal::SampleRate;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use lazy_static::lazy_static;
use parking_lot::Mutex;
use quanta::Instant;
use rand::rngs::StdRng;
use rand::SeedableRng;
use rodio::{OutputStream, Sink};
use rodio::source::{WhiteNoise, Source};
use tokio::sync::mpsc;

mod scavnet;
use scavnet::director::Director;
use scavnet::interface::MainInterface;
use scavnet::scanner::Scanner;
use scavnet::settings::{init_settings, get_volumes};
use scavnet::system::System;
use scavnet::transmission::core::Transmission;

lazy_static! {
    static ref LAST_KEYEVENT: Mutex<KeyEvent> = Mutex::new(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE));
    static ref SETTINGS: Mutex<Config> = Mutex::new(Config::builder().build().unwrap());
}

#[tokio::main]
async fn main() {
    let rng = StdRng::from_entropy();

    let (screen_redraw_rate, debug) = init_settings();
    let frame_time = Duration::from_secs_f64(1.0 / screen_redraw_rate as f64);
    let mut last_frame_time = Instant::now();

    let (signal_tx, signal_rx): (mpsc::Sender<()>, mpsc::Receiver<()>) = mpsc::channel(16);
    let (queue_tx, mut queue_rx): (mpsc::Sender<Transmission>, mpsc::Receiver<Transmission>) = mpsc::channel(16);

    let (_stream, stream_handle) = OutputStream::try_default().unwrap();

    let director_result = Director::new(rng.clone()).await;
    let mut director = match director_result {
        Ok(director) => director,
        Err(e) => {
            eprintln!("Error initializing Director: {}", e);
            return;
        }
    };

    tokio::spawn(transmission_builder(signal_rx, queue_tx, director.clone()));

    let mut scanner = Scanner::new(director.get_networks().clone());
    let mut cycles: u128 = 0;
    let mut system = System::new();
    let mut interface = MainInterface::new();

    if debug {
        system.debug_log("Debug mode enabled.".to_string());
    }

    scanner.start();
    system.log("Connecting to Antenna...".to_string());
    interface.draw(&scanner, &system);

    // Long-running inits here?
    system.log("Ready!".to_string());

    while !interface.get_exit() {
        handle_key_events_thread();
        interface.react_to_key_events();

        if let Some(current_freq) = scanner.next_freq() {
            director.queue.transmissions.retain(|trans| {
                if trans.frequency == current_freq {
                    system.log(format!("Signal Detected! Frequency: {}.", scanner.cur_freq_display()));
                    let (transmission_volume, hiss_volume) = get_volumes();
                    scanner.pause_for_playback();

                    // Play the transmission.
                    let sink = Sink::try_new(&stream_handle).unwrap();

                    let mut items_iter = trans.items.iter();
                    cycles = play_hiss(trans.hiss_preroll, hiss_volume, &sink, &mut scanner, &mut interface, &mut system, cycles, screen_redraw_rate);

                    while let Some(item) = items_iter.next() {
                        system.debug_log(format!("Playing transmission item: {}", item.id));

                        let cursor = std::io::Cursor::new(item.file_bytes.clone());
                        let source = rodio::Decoder::new(cursor).unwrap();

                        sink.set_volume(transmission_volume);
                        sink.append(source);

                        cycles = update_fft_data_during_playback(&sink, item.duration, &item.fft_data, &mut scanner, &mut system, cycles, &mut interface, screen_redraw_rate);

                        if item.sleep_after > 0.0 {
                            cycles = play_hiss(item.sleep_after, hiss_volume, &sink, &mut scanner, &mut interface, &mut system, cycles, screen_redraw_rate);
                        }
                    }

                    cycles = play_hiss(trans.hiss_postroll, hiss_volume, &sink, &mut scanner, &mut interface, &mut system, cycles, screen_redraw_rate);
                    system.log(format!("Signal Lost on frequency {}", scanner.cur_freq_display()));
                    scanner.resume_after_playback();
                    false
                } else {
                    true
                }
            });
        }

        if last_frame_time.elapsed() >= frame_time {

            while let Ok(data) = queue_rx.try_recv() {
                system.debug_log(format!("Received new transmission: {}", data.id));
                director.queue.add(data);
            }

            scanner.simulate_noise();

            if director.needs_queueing() {
                system.debug_log("Queueing new transmission.".to_string());
                if let Err(_) = signal_tx.try_send(()) {
                    panic!("Failed to signal builder.");
                }
                director.set_next_queue_time();
            }

            interface.draw(&scanner, &system);
            last_frame_time = Instant::now();
        }
        cycles += 1;
    }

    // Loop here.
    interface.cleanup();

    // Remove when releasing.
    println!("Cycles: {}", cycles);
}

fn handle_key_events_thread() {
    thread::spawn(|| {
        let _ = handle_key_events();
    });
}

fn handle_key_events() -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(20))? {
        if let Event::Key(key) = event::read()? {
            *LAST_KEYEVENT.lock() = key.clone();
        }
    }
    Ok(false)
}

async fn transmission_builder(
    mut signal_rx: mpsc::Receiver<()>,
    queue_tx: mpsc::Sender<Transmission>,
    director: Director,
) {

    while let Some(_) = signal_rx.recv().await {
        let data = director.get_new_transmission().await;
        if queue_tx.send(data).await.is_err() {
            println!("Main loop dropped, exiting background task.");
            break;
        }

    }
}

// Move these to a separate modules - scanner?
fn update_fft_data_during_playback(sink: &Sink, file_duration: f32, fft_data: &Vec<Vec<f32>>, scanner: &mut Scanner, system: &mut System, cycles: u128, interface: &mut MainInterface, screen_redraw_rate: u128) -> u128{
    let start_time = Instant::now();
    let count_fft_data_points = fft_data.len();
    let duration_per_fft_data_point = file_duration / count_fft_data_points as f32;
    let mut next_fft_update_time = Some(start_time.add(Duration::from_secs_f32(duration_per_fft_data_point)));
    let mut fft_data_index = 0;
    let mut cycles = cycles;

    while !sink.empty() {
        handle_key_events_thread();
        interface.react_to_key_events();

        if cycles % screen_redraw_rate == 0 {
            let now = Instant::now();
            if let Some(next_update_time) = next_fft_update_time {
                if now >= next_update_time {
                    scanner.update_fft_data(fft_data[fft_data_index].clone());
                    fft_data_index += 1;

                    if fft_data_index <= count_fft_data_points {
                        next_fft_update_time = Some(now.add(Duration::from_secs_f32(duration_per_fft_data_point)));
                    } else {
                        next_fft_update_time = None;
                    }
                }
            }
            interface.draw(&scanner, &system);
        }
        cycles += 1;
    }

    return cycles;
}

fn play_hiss(hiss_time: f32, hiss_volume: f32, sink: &Sink, scanner: &mut Scanner, interface: &mut MainInterface, system: &mut System, cycles: u128, screen_redraw_rate: u128) -> u128 {
    let mut local_cycles = cycles;
    let white_noise_source = WhiteNoise::new(SampleRate(44100));
    let hiss_millisecs =  (hiss_time * 1000.0) as u64;
    let white_noise = white_noise_source.take_duration(Duration::from_millis(hiss_millisecs));
    sink.set_volume(hiss_volume);

    system.debug_log(format!("Generating hiss for {} seconds.", hiss_time));
    sink.append(white_noise);

    while !sink.empty() {
        handle_key_events_thread();
        if local_cycles % screen_redraw_rate == 0 {
            scanner.simulate_hiss_noise();
            interface.draw(&scanner, &system);
        }
        local_cycles += 1;
    }

    local_cycles
}
