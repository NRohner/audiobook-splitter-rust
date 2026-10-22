// Local speech-to-text with Whisper (whisper.cpp, Metal GPU on Apple Silicon).
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

pub const MODEL_FILE: &str = "ggml-base.en.bin";

// `$WHISPER_MODEL`, else models/<MODEL_FILE> in the working directory or the repo
// the binary was built in (target/{debug,release}/../../models).
pub fn find_model() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("WHISPER_MODEL") {
        return Ok(PathBuf::from(path));
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let candidates = [
        Some(PathBuf::from("models")),
        exe_dir.clone().map(|d| d.join("models")),
        exe_dir.map(|d| d.join("../../models")),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|dir| dir.join(MODEL_FILE))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("Whisper model '{}' not found. Download it into models/ (`make model`, or see the README on Windows).", MODEL_FILE))
}

pub struct Transcriber {
    state: WhisperState,
}

impl Transcriber {
    pub fn load(model: &Path) -> Result<Self, String> {
        whisper_rs::install_logging_hooks(); // Keep whisper.cpp's logs out of the terminal.
        let model_str = model.to_str().ok_or("Model path is not valid UTF-8")?;
        let ctx = WhisperContext::new_with_params(model_str, WhisperContextParameters::default())
            .map_err(|e| format!("Failed to load Whisper model '{}': {}", model.display(), e))?;
        let state = ctx.create_state().map_err(|e| format!("Failed to init Whisper: {}", e))?;
        Ok(Self { state })
    }

    // Transcribes 16 kHz mono samples.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_no_context(true); // Each clip is independent.
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        self.state.full(params, samples).map_err(|e| format!("Whisper failed: {}", e))?;
        Ok(self.state.as_iter().map(|seg| seg.to_string()).collect::<Vec<_>>().join(" "))
    }
}

// Decodes `duration` seconds of `input` starting at `start` into 16 kHz mono f32 samples.
pub fn clip_samples(input: &Path, start: f64, duration: f64) -> Result<Vec<f32>, String> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-ss", &start.max(0.0).to_string(), "-t", &duration.to_string(), "-i"])
        .arg(input)
        .args(["-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;
    if !output.status.success() {
        return Err(format!("FFmpeg failed to decode clip:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(output.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::{bigram_score, normalize};

    // Needs the model (`make model`) and macOS `say`: cargo test -- --ignored
    #[test]
    #[ignore]
    fn transcribes_speech_close_to_text() {
        let text = "It was a bright cold day in April, and the clocks were striking thirteen. \
                    Winston Smith slipped quickly through the glass doors of Victory Mansions.";
        let aiff = std::env::temp_dir().join(format!("abs-say-{}.aiff", std::process::id()));
        assert!(Command::new("say").arg("-o").arg(&aiff).arg(text).status().unwrap().success());

        let samples = clip_samples(&aiff, 0.0, 30.0).unwrap();
        assert!(samples.len() > 16000);
        let mut t = Transcriber::load(&find_model().unwrap()).unwrap();
        let started = std::time::Instant::now();
        let transcript = t.transcribe(&samples).unwrap();
        println!("{:?} in {:?}", transcript, started.elapsed());
        let score = bigram_score(&normalize(&transcript), &normalize(text));
        assert!(score > 0.7, "score {} for {:?}", score, transcript);
    }
}
