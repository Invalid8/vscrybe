use std::path::Path;
use std::sync::Once;

use ffmpeg::format::Sample;
use ffmpeg::format::sample::Type;
use ffmpeg::software::resampling;
use ffmpeg::{ChannelLayout, frame, media};
use ffmpeg_next as ffmpeg;

use super::Error;

pub const SAMPLE_RATE: u32 = 16_000;

fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if let Err(error) = ffmpeg::init() {
            log::error!("Couldn't initialise FFmpeg: {error}");
        }
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
    });
}

fn open(path: &Path) -> Result<ffmpeg::format::context::Input, Error> {
    init();
    if !path.is_file() {
        return Err(Error::Unreadable(path.into()));
    }
    ffmpeg::format::input(path).map_err(|e| Error::Undecodable(e.to_string()))
}

pub fn decode(path: &Path) -> Result<Vec<f32>, Error> {
    let mut input = open(path)?;
    decode_input(&mut input).map_err(|e| Error::Undecodable(e.to_string()))
}

fn decode_input(input: &mut ffmpeg::format::context::Input) -> Result<Vec<f32>, ffmpeg::Error> {
    let stream = input.streams().best(media::Type::Audio).ok_or(ffmpeg::Error::StreamNotFound)?;
    let index = stream.index();
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?.decoder().audio()?;
    let mut mono = Mono::default();
    let mut rejected = 0;
    let mut first_rejection = None;
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        if let Err(error) = decoder.send_packet(&packet).and_then(|()| mono.drain(&mut decoder)) {
            rejected += 1;
            first_rejection.get_or_insert(error);
        }
    }
    decoder.send_eof()?;
    mono.drain(&mut decoder)?;
    let samples = mono.finish()?;
    if let Some(error) = first_rejection {
        if samples.is_empty() {
            return Err(error);
        }
        log::info!("Skipped {rejected} undecodable packet(s): {error}");
    }
    Ok(samples)
}

#[derive(Default)]
struct Mono {
    resampler: Option<resampling::Context>,
    samples: Vec<f32>,
}

impl Mono {
    fn drain(&mut self, decoder: &mut ffmpeg::decoder::Audio) -> Result<(), ffmpeg::Error> {
        let mut decoded = frame::Audio::empty();
        loop {
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => self.push(&mut decoded)?,
                Err(ffmpeg::Error::Eof) => return Ok(()),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    fn push(&mut self, decoded: &mut frame::Audio) -> Result<(), ffmpeg::Error> {
        if decoded.channel_layout().is_empty() {
            decoded.set_channel_layout(ChannelLayout::default(i32::from(decoded.channels())));
        }
        let resampler = match self.resampler.as_mut() {
            Some(resampler) => resampler,
            None => self.resampler.insert(resampling::Context::get(
                decoded.format(),
                decoded.channel_layout(),
                decoded.rate(),
                Sample::F32(Type::Packed),
                ChannelLayout::MONO,
                SAMPLE_RATE,
            )?),
        };
        let mut out = output_frame(resampler, decoded.samples());
        resampler.run(decoded, &mut out)?;
        self.samples.extend_from_slice(out.plane::<f32>(0));
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<f32>, ffmpeg::Error> {
        if let Some(resampler) = self.resampler.as_mut() {
            loop {
                let mut out = output_frame(resampler, 0);
                resampler.flush(&mut out)?;
                if out.samples() == 0 {
                    break;
                }
                self.samples.extend_from_slice(out.plane::<f32>(0));
            }
        }
        Ok(self.samples)
    }
}

fn output_frame(resampler: &resampling::Context, incoming: usize) -> frame::Audio {
    let pending = resampler.delay().map_or(0, |d| d.input.max(0) as usize);
    let input_rate = resampler.input().rate.max(1) as usize;
    let capacity = (pending + incoming) * SAMPLE_RATE as usize / input_rate + 32;
    frame::Audio::new(Sample::F32(Type::Packed), capacity, ChannelLayout::MONO)
}

pub fn probe_duration(path: &Path) -> Option<f64> {
    let input = open(path).ok()?;
    let duration = input.duration();
    if duration > 0 {
        return Some(duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE));
    }
    decode(path).ok().map(|s| s.len() as f64 / f64::from(SAMPLE_RATE))
}
