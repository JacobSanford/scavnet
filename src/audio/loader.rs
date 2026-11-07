//! Background loading of a transmission's audio bytes and spectrum profiles.
//!
//! The engine chooses what to air; the loader only does I/O. A failed or panicking load
//! comes back as an error for that ticket, and the loader keeps serving later requests.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use super::spectrum::Spectrum;

pub struct LoadRequest {
    pub ticket: u64,
    pub files: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct LoadedClip {
    pub bytes: Arc<[u8]>,
    pub spectrum: Spectrum,
}

pub struct LoadResult {
    pub ticket: u64,
    /// One clip per requested file, in order.
    pub clips: Result<Vec<LoadedClip>, String>,
}

pub trait Loader {
    fn request(&mut self, request: LoadRequest) -> Result<(), String>;
    fn poll(&mut self) -> Option<LoadResult>;
}

/// Runs loads on a dedicated thread. Dropping it ends the thread once its current load finishes.
pub struct ThreadLoader {
    requests: Sender<LoadRequest>,
    results: Receiver<LoadResult>,
}

impl ThreadLoader {
    pub fn spawn(cache_dir: PathBuf) -> std::io::Result<Self> {
        let (requests, request_rx) = mpsc::channel::<LoadRequest>();
        let (result_tx, results) = mpsc::channel();
        std::thread::Builder::new().name("loader".into()).spawn(move || {
            for request in request_rx {
                let clips = load(&request.files, &cache_dir);
                if result_tx
                    .send(LoadResult {
                        ticket: request.ticket,
                        clips,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })?;
        Ok(Self { requests, results })
    }
}

impl Loader for ThreadLoader {
    fn request(&mut self, request: LoadRequest) -> Result<(), String> {
        self.requests
            .send(request)
            .map_err(|_| "loader thread has stopped".to_string())
    }

    fn poll(&mut self) -> Option<LoadResult> {
        self.results.try_recv().ok()
    }
}

/// Loads every file, sharing work when a conversation reuses a clip.
pub fn load(files: &[PathBuf], cache_dir: &Path) -> Result<Vec<LoadedClip>, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut seen: HashMap<&Path, LoadedClip> = HashMap::new();
        files
            .iter()
            .map(|path| {
                if let Some(clip) = seen.get(path.as_path()) {
                    return Ok(clip.clone());
                }
                let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let spectrum = Spectrum::load_or_analyze(path, cache_dir).map_err(|e| format!("{e:#}"))?;
                let clip = LoadedClip {
                    bytes: bytes.into(),
                    spectrum,
                };
                seen.insert(path, clip.clone());
                Ok(clip)
            })
            .collect()
    }))
    .unwrap_or_else(|_| Err("loader panicked".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_support::write_wav;
    use std::time::{Duration, Instant};

    #[test]
    fn thread_loader_returns_results_and_survives_errors() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        write_wav(&wav, 0.25, 8000, 1);
        let mut loader = ThreadLoader::spawn(dir.path().join("cache")).unwrap();
        loader
            .request(LoadRequest {
                ticket: 1,
                files: vec![dir.path().join("missing.wav")],
            })
            .unwrap();
        loader
            .request(LoadRequest {
                ticket: 2,
                files: vec![wav.clone(), wav],
            })
            .unwrap();

        let mut results = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while results.len() < 2 && Instant::now() < deadline {
            match loader.poll() {
                Some(r) => results.push(r),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].ticket, 1);
        match &results[0].clips {
            Err(e) => assert!(e.contains("missing.wav"), "{e}"),
            Ok(_) => panic!("missing file should fail"),
        }
        let Ok(clips) = &results[1].clips else {
            panic!("load should succeed")
        };
        assert_eq!(clips.len(), 2);
        assert!(
            Arc::ptr_eq(&clips[0].bytes, &clips[1].bytes),
            "duplicate files are loaded once"
        );
    }
}
