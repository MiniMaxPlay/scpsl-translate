use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use rtrb::Consumer;

use crate::{AudioError, CAPTURE_CHANNELS, CAPTURE_SAMPLE_RATE};

#[derive(Debug, Default, Clone, Copy)]
pub struct RecordStats {
    pub samples: u64,
    pub overflows: u64,
}

pub fn spawn_recorder(
    path: impl AsRef<Path> + Send + 'static,
    mut consumer: Consumer<f32>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<Result<RecordStats>> {
    let path = path.as_ref().to_owned();
    thread::Builder::new()
        .name("wav-recorder".to_owned())
        .spawn(move || record_loop(&path, &mut consumer, &stop))
        .expect("не удалось создать поток записи WAV")
}

fn record_loop(
    path: &Path,
    consumer: &mut Consumer<f32>,
    stop: &AtomicBool,
) -> Result<RecordStats> {
    let spec = WavSpec {
        channels: CAPTURE_CHANNELS,
        sample_rate: CAPTURE_SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|error| AudioError::Recording(error.to_string()))
        .with_context(|| format!("не удалось создать WAV: {}", path.display()))?;
    let mut stats = RecordStats::default();
    let mut last_flush = Instant::now();
    let mut pending_sample = None;

    loop {
        match consumer.pop() {
            Ok(sample) => {
                if let Some(first) = pending_sample.take() {
                    writer
                        .write_sample(first)
                        .and_then(|_| writer.write_sample(sample))
                        .map_err(|error| AudioError::Recording(error.to_string()))?;
                    stats.samples += 2;
                } else {
                    pending_sample = Some(sample);
                }
            }
            Err(_) if stop.load(Ordering::Acquire) => break,
            Err(_) => thread::sleep(Duration::from_millis(2)),
        }

        if last_flush.elapsed() >= Duration::from_secs(5) {
            writer.flush().context("не удалось сбросить WAV на диск")?;
            last_flush = Instant::now();
        }
    }

    while let Ok(sample) = consumer.pop() {
        if let Some(first) = pending_sample.take() {
            writer
                .write_sample(first)
                .and_then(|_| writer.write_sample(sample))
                .map_err(|error| AudioError::Recording(error.to_string()))?;
            stats.samples += 2;
        } else {
            pending_sample = Some(sample);
        }
    }
    if pending_sample.is_some() {
        tracing::warn!("Неполная стереопара в конце записи отброшена");
    }
    writer
        .finalize()
        .map_err(|error| AudioError::Recording(error.to_string()))?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    use hound::WavReader;
    use rtrb::RingBuffer;

    use super::spawn_recorder;
    use crate::{CAPTURE_CHANNELS, CAPTURE_SAMPLE_RATE};

    #[test]
    fn writes_complete_stereo_frames_to_float_wav() {
        let filename = format!(
            "scpsl-subs-recorder-{}-{}.wav",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(filename);
        let stop = Arc::new(AtomicBool::new(false));
        let (mut producer, consumer) = RingBuffer::<f32>::new(32);
        let expected: Vec<f32> = (0..16).map(|value| value as f32 / 16.0).collect();

        for sample in &expected {
            producer.push(*sample).expect("ring buffer capacity");
        }
        stop.store(true, Ordering::Release);

        let recorder = spawn_recorder(path.clone(), consumer, Arc::clone(&stop));
        let stats = recorder
            .join()
            .expect("recorder thread")
            .expect("recording");
        assert_eq!(stats.samples, expected.len() as u64);

        let reader = WavReader::open(&path).expect("read generated WAV");
        assert_eq!(reader.spec().channels, CAPTURE_CHANNELS);
        assert_eq!(reader.spec().sample_rate, CAPTURE_SAMPLE_RATE);
        assert_eq!(reader.spec().bits_per_sample, 32);
        assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
        assert_eq!(
            reader.duration(),
            (expected.len() / CAPTURE_CHANNELS as usize) as u32
        );

        std::fs::remove_file(path).expect("remove generated WAV");
    }
}
