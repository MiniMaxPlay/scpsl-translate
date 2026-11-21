mod cli;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use rtrb::RingBuffer;
use tracing::{error, info, warn};
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use cli::{Args, SourceArg};
use scpsl_audio::{
    run_capture, spawn_recorder, AudioSource, CaptureStatus, CAPTURE_CHANNELS, CAPTURE_SAMPLE_RATE,
};

fn main() -> Result<()> {
    let args = Args::parse();
    let Some(record_path) = args.record else {
        eprintln!("Ошибка: укажите файл записи, например --record corpus\\voice\\session.wav");
        std::process::exit(2);
    };

    initialize_logging(args.debug)?;
    info!("Запуск записи корпуса");
    info!(
        sample_rate = CAPTURE_SAMPLE_RATE,
        channels = CAPTURE_CHANNELS,
        "Формат записи: 48 кГц, stereo, f32"
    );

    let capacity = CAPTURE_SAMPLE_RATE as usize * CAPTURE_CHANNELS as usize * 10;
    let (producer, consumer) = RingBuffer::<f32>::new(capacity);
    let stop = Arc::new(AtomicBool::new(false));
    let recorder_stop = Arc::clone(&stop);
    let recorder = spawn_recorder(record_path.clone(), consumer, recorder_stop);

    let capture_stop = Arc::clone(&stop);
    let source = match args.source {
        SourceArg::Game => AudioSource::Game {
            process_name: args.game_process,
        },
        SourceArg::System => AudioSource::System,
    };
    let capture = thread::Builder::new()
        .name("audio-capture".to_owned())
        .spawn(move || {
            run_capture(source, producer, capture_stop, |status| match status {
                CaptureStatus::WaitingForGame => info!("Жду запуска SCP: SL"),
                CaptureStatus::Connected => info!("Захват звука подключён"),
                CaptureStatus::Reconnecting => warn!("Переподключение захвата звука"),
            })
        })
        .context("не удалось создать поток захвата")?;

    let signal_stop = Arc::clone(&stop);
    ctrlc::set_handler(move || {
        info!("Получен Ctrl+C, завершение записи");
        signal_stop.store(true, Ordering::Release);
    })
    .context("не удалось зарегистрировать обработчик Ctrl+C")?;

    if let Some(seconds) = args.duration {
        let duration_stop = Arc::clone(&stop);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(seconds));
            info!(seconds, "Длительность записи истекла");
            duration_stop.store(true, Ordering::Release);
        });
    }

    let capture_result = capture
        .join()
        .map_err(|_| anyhow::anyhow!("поток захвата аварийно завершился"))?;
    if let Err(error) = capture_result {
        error!(%error, "Захват завершился с ошибкой");
    }
    stop.store(true, Ordering::Release);

    let stats = recorder
        .join()
        .map_err(|_| anyhow::anyhow!("поток записи аварийно завершился"))??;
    info!(
        samples = stats.samples,
        "WAV сохранён: {}",
        record_path.display()
    );
    Ok(())
}

fn initialize_logging(debug: bool) -> Result<()> {
    let directory = std::path::Path::new("logs");
    std::fs::create_dir_all(directory).context("не удалось создать папку logs")?;
    let file_appender = rolling::daily(directory, "tech.log");
    let filter = if debug { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(fmt::layer().with_target(false))
        .with(fmt::layer().with_ansi(false).with_writer(file_appender))
        .try_init()
        .context("не удалось инициализировать логирование")?;
    Ok(())
}
