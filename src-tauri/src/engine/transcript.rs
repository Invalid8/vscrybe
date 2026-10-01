use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub language: String,
    pub duration: f64,
    pub segments: Vec<Segment>,
}

impl Transcript {
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    pub fn timestamped(&self) -> String {
        self.segments.iter().map(|s| format!("[{}] {}", clock(s.start), s.text)).collect::<Vec<_>>().join("\n")
    }
}

pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 { format!("{hours}:{minutes:02}:{secs:02}") } else { format!("{minutes}:{secs:02}") }
}

pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_formats_minutes_and_hours() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(11.9), "0:11");
        assert_eq!(clock(3725.0), "1:02:05");
    }

    #[test]
    fn text_and_timestamped() {
        let t = Transcript {
            language: "en".into(),
            duration: 70.0,
            segments: vec![
                Segment { start: 0.0, end: 2.0, text: "Hello.".into() },
                Segment { start: 65.0, end: 70.0, text: "Bye.".into() },
            ],
        };
        assert_eq!(t.text(), "Hello. Bye.");
        assert_eq!(t.timestamped(), "[0:00] Hello.\n[1:05] Bye.");
    }
}
