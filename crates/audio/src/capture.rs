use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use rtrb::Producer;
use tracing::{info, warn};
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use crate::process::{find_pid, ProcessWatch};

pub const CAPTURE_SAMPLE_RATE: u32 = 48_000;
pub const CAPTURE_CHANNELS: u16 = 2;
const CAPTURE_BITS: usize = 32;
const BLOCK_DURATION_HNS: i64 = 100_000;

#[derive(Clone, Debug)]
pub enum AudioSource {
    Game { process_name: String },
    System,
}

#[derive(Clone, Debug)]
pub enum CaptureStatus {
    WaitingForGame,
    Connected,
    Reconnecting,
}

pub fn run_capture<F>(
    source: AudioSource,
    mut producer: Producer<f32>,
    stop: Arc<AtomicBool>,
    status: F,
) -> Result<usize>
where
    F: Fn(CaptureStatus) + Send + Sync,
{
    wasapi::initialize_mta()
        .ok()
        .context("не удалось инициализировать COM MTA")?;

    let result = match source {
        AudioSource::Game { process_name } => {
            run_game_capture(&process_name, &mut producer, &stop, &status)
        }
        AudioSource::System => run_system_capture(&mut producer, &stop, &status),
    };

    wasapi::deinitialize();
    result
}

fn run_game_capture<F>(
    process_name: &str,
    producer: &mut Producer<f32>,
    stop: &AtomicBool,
    status: &F,
) -> Result<usize>
where
    F: Fn(CaptureStatus) + Send + Sync,
{
    let mut pushed = 0usize;
    let mut last_overflow_log = Instant::now() - Duration::from_secs(11);

    while !stop.load(Ordering::Acquire) {
        let Some(pid) = find_pid(process_name) else {
            status(CaptureStatus::WaitingForGame);
            thread::sleep(Duration::from_secs(2));
            continue;
        };

        info!(
            pid,
            process = process_name,
            "Игра найдена, подключение к захвату"
        );
        let process_watch = ProcessWatch::open(pid)
            .map_err(|error| anyhow!(error))
            .context("не удалось открыть дескриптор процесса игры")?;
        match capture_client(
            AudioClient::new_application_loopback_client(pid, true)
                .map_err(|error| anyhow!(error))
                .context("не удалось открыть process loopback")?,
            producer,
            stop,
            &mut pushed,
            &mut last_overflow_log,
            status,
        ) {
            Ok(()) if process_watch.is_alive() => break,
            Ok(()) => {
                warn!("Процесс игры завершился, ожидание повторного запуска");
                status(CaptureStatus::Reconnecting);
                thread::sleep(Duration::from_secs(3));
            }
            Err(error) => {
                warn!(%error, "Захват игры прерван, повтор через 3 секунды");
                status(CaptureStatus::Reconnecting);
                thread::sleep(Duration::from_secs(3));
            }
        }
    }

    Ok(pushed)
}

fn run_system_capture<F>(
    producer: &mut Producer<f32>,
    stop: &AtomicBool,
    status: &F,
) -> Result<usize>
where
    F: Fn(CaptureStatus) + Send + Sync,
{
    let enumerator = DeviceEnumerator::new().map_err(|error| anyhow!(error))?;
    let device = enumerator
        .get_default_device(Direction::Render)
        .map_err(|error| anyhow!(error))
        .context("не удалось открыть устройство вывода по умолчанию")?;
    let client = device
        .get_iaudioclient()
        .map_err(|error| anyhow!(error))
        .context("не удалось создать системный loopback-клиент")?;
    let mut pushed = 0usize;
    let mut last_overflow_log = Instant::now() - Duration::from_secs(11);
    capture_client(
        client,
        producer,
        stop,
        &mut pushed,
        &mut last_overflow_log,
        status,
    )?;
    Ok(pushed)
}

fn capture_client<F>(
    mut client: AudioClient,
    producer: &mut Producer<f32>,
    stop: &AtomicBool,
    pushed: &mut usize,
    last_overflow_log: &mut Instant,
    status: &F,
) -> Result<()>
where
    F: Fn(CaptureStatus) + Send + Sync,
{
    let format = WaveFormat::new(
        CAPTURE_BITS,
        CAPTURE_BITS,
        SampleType::Float,
        CAPTURE_SAMPLE_RATE as usize,
        CAPTURE_CHANNELS as usize,
        None,
    );
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: BLOCK_DURATION_HNS,
    };
    client
        .initialize_client(&format, Direction::Capture, mode, None)
        .map_err(|error| anyhow!(error))
        .context("не удалось инициализировать аудиопоток")?;
    let event = client
        .set_get_eventhandle()
        .map_err(|error| anyhow!(error))
        .context("не удалось создать событие аудиопотока")?;
    let capture = client
        .get_audiocaptureclient()
        .map_err(|error| anyhow!(error))
        .context("не удалось получить capture client")?;
    client
        .start_stream()
        .map_err(|error| anyhow!(error))
        .context("не удалось запустить аудиопоток")?;
    status(CaptureStatus::Connected);

    let block_align = format.get_blockalign() as usize;
    let mut bytes = VecDeque::with_capacity(block_align * 4096);
    while !stop.load(Ordering::Acquire) {
        capture
            .read_from_device_to_deque(&mut bytes)
            .map_err(|error| anyhow!(error))
            .context("ошибка чтения аудиопакета")?;

        while bytes.len() >= block_align {
            let mut sample_bytes = [0u8; 4];
            for byte in &mut sample_bytes {
                *byte = bytes.pop_front().expect("проверен размер буфера");
            }
            let sample = f32::from_le_bytes(sample_bytes);
            if producer.push(sample).is_err() {
                if last_overflow_log.elapsed() >= Duration::from_secs(10) {
                    warn!("Кольцевой буфер звука переполнен: новые сэмплы отброшены");
                    *last_overflow_log = Instant::now();
                }
            } else {
                *pushed += 1;
            }
        }

        if event.wait_for_event(1_000_000).is_err() {
            return Err(anyhow!("событие аудиопотока завершилось"));
        }
    }

    let _ = client.stop_stream();
    Ok(())
}
