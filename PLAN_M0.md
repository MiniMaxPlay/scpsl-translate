
# PLAN M0 — каркас и запись корпуса (для Codex)

## Goal
Cargo workspace + крейт `audio`: захват звука только процесса SCPSL.exe
(WASAPI process loopback) и запись в WAV 48 кГц флагом `--record`.
Результат: можно записывать тестовый корпус (ТЗ: AUD-1/2/3/5/8, CLI-2, разд. 11).

## Prerequisites (вручную, до кода)
- rustup, toolchain `stable-x86_64-pc-windows-msvc`
- VS Build Tools 2022 с C++ (CMake и LLVM — позже, к M1)

## Files to Create
- Cargo.toml                      — workspace, members = crates/*
- rust-toolchain.toml             — channel = "stable"
- rustfmt.toml, .gitignore        — target/, logs/, corpus/, *.wav
- crates/app/Cargo.toml
- crates/app/src/main.rs          — запуск, логи, Ctrl+C
- crates/app/src/cli.rs           — clap
- crates/audio/Cargo.toml
- crates/audio/src/lib.rs         — pub API, AudioError
- crates/audio/src/process.rs     — поиск PID, проверка жив ли процесс
- crates/audio/src/capture.rs     — WASAPI loopback (process / system)
- crates/audio/src/recorder.rs    — rtrb → WAV-писатель
- README.md                       — сборка, --record, лицензии (кратко)

Остальные крейты (voice, text, overlay, config, bench) в M0 НЕ создавать.

## Dependencies
- app: clap (derive), anyhow, tracing, tracing-subscriber, tracing-appender, ctrlc
- audio: wasapi, windows (Win32_Foundation, Win32_System_Threading,
  Win32_System_Diagnostics_ToolHelp, Win32_Media_Audio, Win32_System_Com),
  rtrb, hound, thiserror, tracing

## Steps

1. Workspace: корневой Cargo.toml с `[workspace.dependencies]`, resolver = "2",
   `[profile.release] lto = "thin"`. Крейты ссылаются через `workspace = true`.

2. audio::process
   - `fn find_pid(exe_name: &str) -> Option<u32>` — CreateToolhelp32Snapshot,
     сравнение имени без учёта регистра.
   - `struct ProcessWatch { handle }`, `fn open(pid) -> Result<Self>`,
     `fn is_alive(&self) -> bool` — WaitForSingleObject(handle, 0).
   - unsafe только здесь и в capture.rs (NFR-11).

3. audio::capture
   - `enum Source { Game { exe: String }, System }`
   - `const CAPTURE_FORMAT: 48000 Hz, 2 ch, f32`
   - `fn run_capture(source, producer: rtrb::Producer<f32>, stop: Arc<AtomicBool>,
      status: &dyn Fn(Status)) -> Result<()>`
   - Game: ждать PID, опрос раз в 2 с, статус «Жду запуска SCP: SL» (AUD-2);
     клиент через wasapi application loopback (include process tree = true),
     событийный режим, autoconvert в CAPTURE_FORMAT.
   - System: loopback устройства вывода по умолчанию (AUD-8).
   - Поток захвата: MMCSS «Audio» (AvSetMmThreadCharacteristicsW, NFR-7).
   - Внешний цикл: ошибка потока или смена устройства → лог, пауза 3 с,
     переоткрытие (AUD-3); процесс умер → снова ждать PID, без выхода.
   - Тишина/нет пакетов не ошибка: при отсутствии данных писать нули
     не нужно (WAV содержит только полученные сэмплы).
   - Переполнение rtrb: отбросить новые сэмплы, warn в лог не чаще 1 раза/10 с.

4. audio::recorder
   - `struct WavRecorder`; `fn spawn(path, consumer, stop) -> JoinHandle<Result<RecordStats>>`
   - hound WavSpec { 48000, 2 ch, 32 bit float }.
   - Читать из rtrb пачками, flush() раз в 5 с (данные не теряются при падении).
   - `struct RecordStats { samples: u64, overflows: u64 }`.
   - Ёмкость rtrb: 10 с × 48000 × 2.

5. app::cli (clap)
   - `--record <PATH>` (обязателен в M0)
   - `--source <game|system>` (default game)
   - `--game-process <NAME>` (default "SCPSL.exe")
   - `--duration <SECS>` (опционально, авто-стоп)
   - `--debug`
   - Без `--record` → понятное сообщение на русском и выход с кодом 2.

6. app::main
   - tracing: консоль + logs/tech.log (уровень info, --debug → debug).
   - Все сообщения пользователю и лог — на русском (NFR-12).
   - Ctrl+C / --duration → stop=true → дождаться recorder → finalize WAV.
   - Каждые 10 с в лог: «Запись: N с, переполнений: M».
   - Паника в потоке → join вернёт Err → лог + корректный finalize.

7. README.md: установка инструментов, `cargo build --release`,
   пример записи корпуса, требование «окно без рамки», структура corpus/:
   corpus/voice/*.wav, corpus/noise/*.wav, corpus/stream/*.wav, corpus/labels/*.srt.

## Unit tests
- recorder: запись синтетического синуса → чтение hound → spec и длина совпадают.
- process: find_pid("no_such_process_xyz.exe") == None;
  find_pid текущего exe (std::env::current_exe) == Some(std::process::id()).
- capture: логика переполнения/троттлинга лога вынесена в чистую функцию и протестирована.

## Verification
cargo fmt --all -- --