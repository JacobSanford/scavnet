//! `scavnet generate`: author a conversation with OpenAI, once, into the git-tracked world.
//!
//! Writes one WAV per script line, the prompt used, and a conversation entry appended to the
//! set's set.yaml. The world is re-validated afterwards; on failure every change is rolled back.
//! Nothing here runs during `scavnet run`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde_json::{json, Value};

use crate::settings::Settings;
use crate::world::{World, SET_FILE_NAME};

const API: &str = "https://api.openai.com/v1";
/// OpenAI's `pcm` speech format: 24 kHz, 16-bit signed little-endian, mono.
const PCM_RATE: u32 = 24_000;

#[derive(Args)]
pub struct GenerateArgs {
    /// Set directory as written in sets.yaml (e.g. `signals`).
    set: PathBuf,
    /// Id for the new conversation. Also names its files.
    #[arg(long)]
    id: String,
    /// Prompt text.
    #[arg(long, conflicts_with = "prompt_file", required_unless_present = "prompt_file")]
    prompt: Option<String>,
    /// File holding the prompt.
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    /// Chat model that writes the script.
    #[arg(long, default_value = "gpt-4o-mini")]
    model: String,
    /// Speech model.
    #[arg(long, default_value = "tts-1-hd")]
    tts_model: String,
    /// Speech voice.
    #[arg(long, default_value = "alloy")]
    voice: String,
    /// Pick weight under `playback: random`.
    #[arg(long, default_value_t = 1)]
    weight: u32,
    /// Fixed frequency in Hz. Omit for a random frequency per airing.
    #[arg(long)]
    frequency: Option<u64>,
    /// Shortest hiss between lines, in seconds.
    #[arg(long, default_value_t = 1.0)]
    gap_min: f32,
    /// Longest hiss between lines, in seconds.
    #[arg(long, default_value_t = 3.0)]
    gap_max: f32,
}

/// A script turned into audio, ready to be written into a set.
pub struct Generated {
    pub id: String,
    pub description: String,
    pub prompt: String,
    pub weight: u32,
    pub frequency: Option<u64>,
    pub gap: (f32, f32),
    /// (caption, PCM samples) per line.
    pub lines: Vec<(String, Vec<i16>)>,
}

pub fn run(settings: &Settings, args: GenerateArgs) -> Result<()> {
    let library = settings.library_path();
    let set_dir = library.parent().unwrap_or(Path::new("")).join(&args.set);
    let world = World::load(&settings.networks_path(), &library).context("Fix the world before generating into it")?;
    let set_file = set_dir.join(SET_FILE_NAME);
    let set = world
        .sets
        .iter()
        .find(|s| s.file == set_file)
        .with_context(|| format!("{} is not a set in {}", set_file.display(), library.display()))?;
    if set.conversations.iter().any(|c| c.id == args.id) {
        bail!("Set '{}' already has a conversation '{}'", set.name, args.id);
    }
    if !(0.0..=args.gap_max).contains(&args.gap_min) {
        bail!("--gap-min must be between 0 and --gap-max");
    }

    let prompt = match (&args.prompt, &args.prompt_file) {
        (Some(p), _) => p.clone(),
        (None, Some(f)) => std::fs::read_to_string(f).with_context(|| format!("Failed to read {}", f.display()))?,
        (None, None) => unreachable!("clap requires one of them"),
    };
    let key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into();

    eprintln!("Writing script with {}...", args.model);
    let script = chat(&agent, &key, &args.model, &prompt)?;
    let captions = script_lines(&script);
    if captions.is_empty() {
        bail!("The model returned an empty script:\n{script}");
    }
    let mut lines = Vec::with_capacity(captions.len());
    for (i, caption) in captions.into_iter().enumerate() {
        eprintln!("Speaking line {}: {caption}", i + 1);
        let pcm = speech(&agent, &key, &args.tts_model, &args.voice, &caption)?;
        lines.push((caption, pcm));
    }

    let generated = Generated {
        description: format!("Generated with {} and {} ({}).", args.model, args.tts_model, args.voice),
        id: args.id,
        prompt,
        weight: args.weight,
        frequency: args.frequency,
        gap: (args.gap_min, args.gap_max),
        lines,
    };
    write_into_set(&generated, &set_dir, &settings.networks_path(), &library)?;
    eprintln!("Added conversation '{}' to {}", generated.id, set_file.display());
    Ok(())
}

fn post(agent: &ureq::Agent, key: &str, path: &str, body: &Value) -> Result<ureq::http::Response<ureq::Body>> {
    let mut response = agent
        .post(format!("{API}/{path}"))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(body)
        .with_context(|| format!("Request to {path} failed"))?;
    if !response.status().is_success() {
        let detail = response.body_mut().read_to_string().unwrap_or_default();
        bail!("{path} returned {}: {detail}", response.status());
    }
    Ok(response)
}

fn chat(agent: &ureq::Agent, key: &str, model: &str, prompt: &str) -> Result<String> {
    let body = json!({ "model": model, "messages": [{ "role": "user", "content": prompt }] });
    let reply: Value = post(agent, key, "chat/completions", &body)?.body_mut().read_json()?;
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("Unexpected chat response: {reply}"))
}

fn speech(agent: &ureq::Agent, key: &str, model: &str, voice: &str, text: &str) -> Result<Vec<i16>> {
    let body = json!({ "model": model, "voice": voice, "input": text, "response_format": "pcm" });
    let bytes = post(agent, key, "audio/speech", &body)?
        .body_mut()
        .with_config()
        .limit(200 << 20)
        .read_to_vec()?;
    Ok(bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect())
}

/// Non-empty lines with surrounding quotes removed. Each becomes one spoken item.
pub fn script_lines(script: &str) -> Vec<String> {
    script
        .lines()
        .map(|l| {
            l.trim()
                .trim_matches(|c| matches!(c, '"' | '\u{201c}' | '\u{201d}'))
                .trim()
        })
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Writes WAVs, the prompt and the YAML entry, then re-validates; rolls back on any failure.
pub fn write_into_set(g: &Generated, set_dir: &Path, networks: &Path, library: &Path) -> Result<()> {
    let set_file = set_dir.join(SET_FILE_NAME);
    let original = std::fs::read_to_string(&set_file)?;
    let mut created: Vec<PathBuf> = Vec::new();

    let result = (|| -> Result<()> {
        let mut files = Vec::new();
        for (n, (_, pcm)) in g.lines.iter().enumerate() {
            let name = format!("{}-{}.wav", g.id, n + 1);
            let path = set_dir.join(&name);
            if path.exists() {
                bail!("{} already exists", path.display());
            }
            created.push(path.clone());
            write_wav(&path, pcm)?;
            files.push(name);
        }
        let prompt_path = set_dir.join("prompts").join(format!("{}.txt", g.id));
        if prompt_path.exists() {
            bail!("{} already exists", prompt_path.display());
        }
        std::fs::create_dir_all(prompt_path.parent().unwrap_or(set_dir))?;
        created.push(prompt_path.clone());
        std::fs::write(&prompt_path, &g.prompt)?;

        std::fs::write(&set_file, append_conversation(&original, g, &files)?)?;
        World::load(networks, library).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::write(&set_file, &original);
        for path in created {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn write_wav(path: &Path, pcm: &[i16]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: PCM_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in pcm {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Appends a conversation to the text of set.yaml, matching its list indentation.
/// Text is appended rather than re-serialized so authors' comments and layout survive.
fn append_conversation(original: &str, g: &Generated, files: &[String]) -> Result<String> {
    let after_key = original
        .find("\nconversations:")
        .map(|i| &original[i..])
        .context("set.yaml has no top-level 'conversations:' key")?;
    let indent: String = after_key
        .lines()
        .skip(1)
        .find(|l| l.trim_start().starts_with("- "))
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .unwrap_or_else(|| "  ".to_string());
    let q = |s: &str| serde_json::to_string(s).expect("strings always serialize");

    let mut out = original.trim_end().to_string();
    out.push_str(&format!("\n\n{indent}- id: {}\n", q(&g.id)));
    out.push_str(&format!("{indent}  description: {}\n", q(&g.description)));
    out.push_str(&format!("{indent}  weight: {}\n", g.weight));
    if let Some(f) = g.frequency {
        out.push_str(&format!("{indent}  frequency: {f}\n"));
    }
    out.push_str(&format!("{indent}  items:\n"));
    for (n, ((caption, _), file)) in g.lines.iter().zip(files).enumerate() {
        out.push_str(&format!("{indent}    - id: {}\n", q(&format!("{}-{}", g.id, n + 1))));
        out.push_str(&format!("{indent}      caption: {}\n", q(caption)));
        out.push_str(&format!("{indent}      file: {}\n", q(file)));
        out.push_str(&format!(
            "{indent}      gap_after: {{ min: {}, max: {} }}\n",
            g.gap.0, g.gap.1
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_support::sample_world;

    fn generated(id: &str) -> Generated {
        Generated {
            id: id.into(),
            description: "Generated for a test.".into(),
            prompt: "Say \"hi\"".into(),
            weight: 3,
            frequency: None,
            gap: (1.0, 2.0),
            lines: vec![
                ("They're: \"everywhere\".".into(), vec![1000; 2400]),
                ("Run.".into(), vec![-1000; 4800]),
            ],
        }
    }

    #[test]
    fn script_lines_strip_quotes_and_blanks() {
        assert_eq!(
            script_lines("\"Need water.\n\n  Anyone?\n\u{201c}North tower.\u{201d}\n"),
            ["Need water.", "Anyone?", "North tower."]
        );
    }

    #[test]
    fn writes_valid_conversation_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let (net, lib) = sample_world(dir.path());
        let set_dir = dir.path().join("transmissions/noise");
        let set_file = set_dir.join(SET_FILE_NAME);
        let with_comment = format!("# keep me\n{}", std::fs::read_to_string(&set_file).unwrap());
        std::fs::write(&set_file, &with_comment).unwrap();

        write_into_set(&generated("gen"), &set_dir, &net, &lib).unwrap();

        let world = World::load(&net, &lib).unwrap();
        let noise = world.sets.iter().find(|s| s.name == "noise").unwrap();
        let conv = noise.conversations.iter().find(|c| c.id == "gen").unwrap();
        assert_eq!(conv.weight, 3);
        assert_eq!(conv.items.len(), 2);
        assert_eq!(conv.items[0].caption, "They're: \"everywhere\".");
        assert!((conv.items[1].duration - 0.2).abs() < 1e-3);
        assert!(std::fs::read_to_string(&set_file).unwrap().starts_with("# keep me\n"));
        assert_eq!(
            std::fs::read_to_string(set_dir.join("prompts/gen.txt")).unwrap(),
            "Say \"hi\""
        );
    }

    #[test]
    fn invalid_result_is_rolled_back() {
        let dir = tempfile::tempdir().unwrap();
        let (net, lib) = sample_world(dir.path());
        let set_dir = dir.path().join("transmissions/noise");
        let before = std::fs::read_to_string(set_dir.join(SET_FILE_NAME)).unwrap();
        let mut g = generated("gen");
        g.frequency = Some(1505); // off the 10 Hz grid
        let err = write_into_set(&g, &set_dir, &net, &lib).unwrap_err().to_string();
        assert!(err.contains("1505"), "{err}");
        assert_eq!(std::fs::read_to_string(set_dir.join(SET_FILE_NAME)).unwrap(), before);
        assert!(!set_dir.join("gen-1.wav").exists());
        assert!(!set_dir.join("prompts/gen.txt").exists());
    }
}
