use std::path::Path;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

use super::Error;

pub const SAMPLE_RATE: u32 = 16_000;
const STALL: Duration = Duration::from_secs(30);

pub fn decode(path: &Path) -> Result<Vec<f32>, Error> {
    let undecodable = |error: &dyn std::fmt::Display| Error::Undecodable(error.to_string());
    gst::init().map_err(|e| undecodable(&e))?;
    let uri = gst::glib::filename_to_uri(std::path::absolute(path).unwrap_or_else(|_| path.into()), None)
        .map_err(|_| Error::Unreadable(path.into()))?;
    let pipeline = gst::parse::launch(&format!(
        "uridecodebin uri=\"{uri}\" caps=audio/x-raw expose-all-streams=false ! audioconvert ! audioresample ! \
         audio/x-raw,format=F32LE,channels=1,rate={SAMPLE_RATE},layout=interleaved ! appsink name=sink sync=false"
    ))
    .map_err(|e| undecodable(&e))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| undecodable(&"the decoder isn't a pipeline"))?;
    let outcome = pull_samples(&pipeline).map_err(|e| undecodable(&e));
    let _ = pipeline.set_state(gst::State::Null);
    outcome
}

fn pull_samples(pipeline: &gst::Pipeline) -> Result<Vec<f32>, String> {
    let sink = pipeline.by_name("sink").and_downcast::<AppSink>().ok_or("the decoder has no audio sink")?;
    let bus = pipeline.bus().ok_or("the decoder has no message bus")?;
    pipeline.set_state(gst::State::Playing).map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let mut last_sample = Instant::now();
    loop {
        if let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
            last_sample = Instant::now();
            if let Some(buffer) = sample.buffer() {
                let map = buffer.map_readable().map_err(|e| e.to_string())?;
                samples.extend(map.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            }
        } else if sink.is_eos() {
            return Ok(samples);
        } else if last_sample.elapsed() > STALL {
            return Err(format!("no audio for {} seconds", STALL.as_secs()));
        }
        if let Some(message) = bus.pop_filtered(&[gst::MessageType::Error])
            && let gst::MessageView::Error(error) = message.view()
        {
            return Err(error.error().to_string());
        }
    }
}

pub fn probe_duration(path: &Path) -> Option<f64> {
    gst::init().ok()?;
    let uri = gst::glib::filename_to_uri(std::path::absolute(path).unwrap_or_else(|_| path.into()), None).ok()?;
    let discovered = gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(10))
        .ok()?
        .discover_uri(&uri)
        .ok()
        .and_then(|info| info.duration())
        .filter(|d| !d.is_zero())
        .map(|d| d.nseconds() as f64 / 1e9);
    discovered.or_else(|| decode(path).ok().map(|s| s.len() as f64 / SAMPLE_RATE as f64))
}

const REQUIRED: &[(&str, &str)] = &[
    ("uridecodebin", "gstreamer1.0-plugins-base"),
    ("audioconvert", "gstreamer1.0-plugins-base"),
    ("audioresample", "gstreamer1.0-plugins-base"),
    ("oggdemux", "gstreamer1.0-plugins-base"),
    ("opusdec", "gstreamer1.0-plugins-base"),
    ("matroskademux", "gstreamer1.0-plugins-good"),
    ("qtdemux", "gstreamer1.0-plugins-good"),
    ("wavparse", "gstreamer1.0-plugins-good"),
    ("avdec_aac", "gstreamer1.0-libav"),
];

pub fn missing_elements() -> Vec<&'static str> {
    if gst::init().is_err() {
        return vec!["libgstreamer1.0-0"];
    }
    let mut packages: Vec<&str> =
        REQUIRED.iter().filter(|(e, _)| gst::ElementFactory::find(e).is_none()).map(|(_, p)| *p).collect();
    packages.dedup();
    packages
}
