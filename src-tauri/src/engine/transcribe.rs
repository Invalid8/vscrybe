use std::ops::Range;
use std::path::Path;

use ct2rs::sys::{StorageView, Whisper};
use ct2rs::{ComputeType, Config, Device, WhisperOptions};
use tokenizers::Tokenizer;

use super::features::{FRAMES, LogMel};
use super::transcript::{Segment, Transcript, round2};
use super::{Error, SAMPLE_RATE, decode, models};

pub const DEFAULT_LANGUAGE: &str = "en";
pub const AUTO: &str = "auto";

pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    (AUTO, "Auto-detect"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
    ("de", "German"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sw", "Swahili"),
    ("yo", "Yoruba"),
    ("ha", "Hausa"),
    ("hi", "Hindi"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
];

pub fn is_language(code: &str) -> bool {
    LANGUAGES.iter().any(|(c, _)| *c == code)
}

const RATE: usize = SAMPLE_RATE as usize;
const FRAME: usize = RATE * 30 / 1000;
const MAX_CHUNK: usize = 30 * RATE;
const MIN_CUT: usize = 20 * RATE;
const BRIDGE: usize = 2 * RATE;
const MERGE_GAP: usize = 5 * RATE;
const PAD: usize = RATE * 3 / 10;

pub struct Engine {
    whisper: Whisper,
    tokenizer: Tokenizer,
    log_mel: LogMel,
    first_timestamp: u32,
    end_of_text: u32,
    pub model: String,
}

impl Engine {
    pub fn load(model: &str) -> Result<Self, String> {
        let folder = models::ensure(model)?;
        let failed = |e: &dyn std::fmt::Display| format!("Couldn't load the {} model: {e}", models::label(model));
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).div_ceil(2);
        let config = Config {
            device: Device::CPU,
            compute_type: ComputeType::INT8,
            num_threads_per_replica: threads,
            ..Default::default()
        };
        let whisper = Whisper::new(&folder, config).map_err(|e| failed(&e))?;
        let tokenizer = Tokenizer::from_file(folder.join("tokenizer.json")).map_err(|e| failed(&e))?;
        let id = |token: &str| tokenizer.token_to_id(token).ok_or_else(|| failed(&format!("no {token} token")));
        Ok(Self {
            first_timestamp: id("<|notimestamps|>")? + 1,
            end_of_text: id("<|endoftext|>")?,
            log_mel: LogMel::load(&folder)?,
            whisper,
            tokenizer,
            model: model.into(),
        })
    }

    pub fn transcribe(&self, path: &Path, language: &str, on_progress: &mut dyn FnMut(f64)) -> Result<Transcript, Error> {
        let samples = decode(path)?;
        let duration = samples.len() as f64 / RATE as f64;
        let chunks = speech_chunks(&samples);
        let language = match (language, chunks.first()) {
            (AUTO, Some(first)) => self.detect(&samples[first.clone()]).map_err(Error::Transcription)?,
            (AUTO, None) => DEFAULT_LANGUAGE.to_string(),
            (code, _) => code.to_string(),
        };
        let mut segments = Vec::new();
        for chunk in chunks {
            let offset = chunk.start as f64 / RATE as f64;
            let length = chunk.len() as f64 / RATE as f64;
            let pieces = self.generate(&samples[chunk.clone()], &language).map_err(Error::Transcription)?;
            segments.extend(segments_from(&pieces, length).into_iter().map(|s| Segment {
                start: round2(offset + s.start),
                end: round2(offset + s.end),
                text: s.text,
            }));
            on_progress((chunk.end as f64 / samples.len() as f64).min(1.0));
        }
        Ok(Transcript { language, duration: round2(duration), segments })
    }

    fn detect(&self, audio: &[f32]) -> Result<String, String> {
        let mut features = self.log_mel.compute(audio);
        let view = StorageView::new(&[1, self.log_mel.n_mels, FRAMES], &mut features, Device::CPU).map_err(|e| e.to_string())?;
        let detected = self.whisper.detect_language(&view).map_err(|e| e.to_string())?;
        let best = detected.into_iter().next().and_then(|d| d.into_iter().next()).ok_or("no language detected")?;
        Ok(best.language.trim_start_matches("<|").trim_end_matches("|>").to_string())
    }

    fn generate(&self, audio: &[f32], language: &str) -> Result<Vec<Piece>, String> {
        let mut features = self.log_mel.compute(audio);
        let view = StorageView::new(&[1, self.log_mel.n_mels, FRAMES], &mut features, Device::CPU).map_err(|e| e.to_string())?;
        let prompt = vec![vec!["<|startoftranscript|>".to_string(), format!("<|{language}|>"), "<|transcribe|>".into()]];
        let options = WhisperOptions { beam_size: 1, ..Default::default() };
        let result = self.whisper.generate(&view, &prompt, &options).map_err(|e| e.to_string())?;
        let ids = result.into_iter().next().and_then(|r| r.sequences_ids.into_iter().next()).unwrap_or_default();
        let mut pieces = Vec::new();
        let mut text: Vec<u32> = Vec::new();
        for id in ids.into_iter().map(|id| id as u32) {
            if id < self.end_of_text {
                text.push(id);
                continue;
            }
            if !text.is_empty() {
                pieces.push(Piece::Text(self.tokenizer.decode(&text, true).map_err(|e| e.to_string())?));
                text.clear();
            }
            if id >= self.first_timestamp {
                pieces.push(Piece::Time(f64::from(id - self.first_timestamp) * 0.02));
            }
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(self.tokenizer.decode(&text, true).map_err(|e| e.to_string())?));
        }
        Ok(pieces)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Time(f64),
    Text(String),
}

fn frame_levels(samples: &[f32]) -> Vec<f32> {
    samples.chunks(FRAME).map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt()).collect()
}

fn speech_threshold(levels: &[f32]) -> f32 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q) as usize];
    (at(0.1) + (at(0.9) - at(0.1)) * 0.1).max(0.002)
}

pub fn speech_chunks(samples: &[f32]) -> Vec<Range<usize>> {
    if samples.is_empty() {
        return Vec::new();
    }
    let levels = frame_levels(samples);
    let threshold = speech_threshold(&levels);
    let loud: Vec<bool> = levels.iter().map(|l| *l >= threshold).collect();

    let mut regions: Vec<Range<usize>> = Vec::new();
    for (i, _) in loud.iter().enumerate().filter(|(_, l)| **l) {
        let (start, end) = ((i * FRAME).saturating_sub(PAD), ((i + 1) * FRAME + PAD).min(samples.len()));
        match regions.last_mut() {
            Some(last) if start <= last.end + BRIDGE => last.end = end,
            _ => regions.push(start..end),
        }
    }
    regions.retain(|r| r.len() >= RATE / 4);

    let mut chunks: Vec<Range<usize>> = Vec::new();
    for region in regions {
        if let Some(last) = chunks.last_mut() {
            if region.start - last.end <= MERGE_GAP && region.end - last.start <= MAX_CHUNK {
                last.end = region.end;
                continue;
            }
        }
        let mut start = region.start;
        while region.end - start > MAX_CHUNK {
            let cut = quietest(&levels, start + MIN_CUT, start + MAX_CHUNK);
            chunks.push(start..cut);
            start = cut;
        }
        chunks.push(start..region.end);
    }
    chunks
}

fn quietest(levels: &[f32], from: usize, to: usize) -> usize {
    let (first, last) = (from / FRAME, (to / FRAME).min(levels.len()));
    let frame = (first..last).min_by(|a, b| levels[*a].total_cmp(&levels[*b])).unwrap_or(first);
    frame * FRAME
}

pub fn segments_from(pieces: &[Piece], length: f64) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut start: Option<f64> = None;
    let mut text = String::new();
    let flush = |start: f64, end: f64, text: &mut String, segments: &mut Vec<Segment>| {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            segments.push(Segment { start, end: end.max(start), text: trimmed.to_string() });
        }
        text.clear();
    };
    for piece in pieces {
        match piece {
            Piece::Text(t) => text.push_str(t),
            Piece::Time(time) => match start {
                Some(s) if !text.trim().is_empty() => {
                    flush(s, *time, &mut text, &mut segments);
                    start = None;
                }
                _ => {
                    text.clear();
                    start = Some(*time);
                }
            },
        }
    }
    flush(start.unwrap_or(0.0), length, &mut text, &mut segments);
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(time: f64) -> Piece {
        Piece::Time(time)
    }

    fn w(text: &str) -> Piece {
        Piece::Text(text.into())
    }

    #[test]
    fn timestamps_bracket_segments() {
        let s = segments_from(&[t(0.0), w(" Hello there."), t(2.4), t(2.4), w(" How are you?"), t(5.0), t(5.2), w(" Trailing")], 8.0);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].start, s[0].end, s[0].text.as_str()), (0.0, 2.4, "Hello there."));
        assert_eq!((s[1].start, s[1].end, s[1].text.as_str()), (2.4, 5.0, "How are you?"));
        assert_eq!((s[2].start, s[2].end, s[2].text.as_str()), (5.2, 8.0, "Trailing"));
    }

    #[test]
    fn plain_output_is_one_segment() {
        let s = segments_from(&[w(" Just text.")], 3.0);
        assert_eq!((s[0].start, s[0].end, s[0].text.as_str()), (0.0, 3.0, "Just text."));
        assert!(segments_from(&[t(0.0), t(1.0)], 1.0).is_empty());
    }

    fn tone(seconds: f64) -> Vec<f32> {
        (0..(seconds * RATE as f64) as usize).map(|i| (i as f32 * 0.05).sin() * 0.3).collect()
    }

    #[test]
    fn silence_has_no_chunks_and_long_speech_is_split() {
        assert!(speech_chunks(&vec![0.0; 10 * RATE]).is_empty());
        let mut audio = vec![0.0; 5 * RATE];
        audio.extend(tone(70.0));
        audio.extend(vec![0.0; 5 * RATE]);
        let chunks = speech_chunks(&audio);
        assert!(chunks.len() >= 3, "{chunks:?}");
        assert!(chunks.iter().all(|c| c.len() <= MAX_CHUNK));
        assert!(chunks[0].start >= 4 * RATE);
        assert!(chunks.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn nearby_speech_shares_a_chunk_and_long_gaps_split() {
        let mut audio = tone(3.0);
        audio.extend(vec![0.0; RATE]);
        audio.extend(tone(3.0));
        audio.extend(vec![0.0; 10 * RATE]);
        audio.extend(tone(3.0));
        assert_eq!(speech_chunks(&audio).len(), 2);
    }
}
