# PLAN_NEXT — состояние проекта и следующие шаги

Дата: 21.11.2026. Документ отвечает на «что осталось и что нужно от пользователя».
Основной план по требованиям — `PLAN_CODEX.md`, этап M0 — `PLAN_M0.md`.

## 0. Ответы пользователя (зафиксированы 21.11.2026)

| Вопрос | Ответ | Следствие |
|---|---|---|
| Ключ ProxyAPI | Ключ есть, вписывать в систему пока не хочет | Сборка и M1-M4 работают без ключа: Haiku включается флагом `--translator haiku`, при отсутствии `PROXYAPI_KEY` — сообщение и режим «только английский» (TXT-1) |
| Модели ASR/VAD | «Скачивай сам, клади в папку проекта» | Качаю `sherpa-onnx` win64 shared и `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` в `vendor/`, папка в `.gitignore` |
| Git и GitHub | Коммит на каждую функцию, заливать на GitHub | Локальный репозиторий и коммит `20facbf` есть; нужен вход в `gh`, потом `gh repo create` и `push` |
| Sherpa-onnx A или B | Не выбран | Решаю сам по ходу M1; по факту отсутствия CMake/LLVM начинаю с варианта A (раздел 4.1) |

## 1. Проверено на 21.11.2026

| Проверка | Команда | Результат |
|---|---|---|
| Форматирование | `cargo fmt --check` | OK |
| Линт | `cargo clippy --workspace --all-targets -- -D warnings` | OK |
| Тесты | `cargo test --workspace` | 3 теста OK (recorder, find_pid ×2) |
| Сборка | `cargo build --release` | OK, exe от 18.11 |
| Живой захват | `--record --source system` | `corpus/system/smoke.wav`, 5 с, 260 094 сэмпла |

Есть: Rust 1.100 stable (msvc), MSVC cl 14.29 (VS 2019 Community), Windows Kit
10.0.17763, Python 3.13, рабочий WASAPI process loopback.

Нет: CMake, LLVM/clang, `PROXYAPI_KEY`, git-репозиторий, sherpa-совместимые модели
(есть только `silero_vad.onnx` v5 из прототипа и faster-whisper base.en/small.en —
для Sherpa не годятся).

## 2. Сделано

- Workspace: `crates/app`, `crates/audio`, общие версии, `lto = "thin"`.
- CLI (`crates/app/src/cli.rs`): `--record`, `--source game|system`,
  `--game-process`, `--duration`, `--debug`. Без `--record` — сообщение на русском, код 2.
- Логи: консоль + `logs/tech.log`.
- `audio::process`: `find_pid` (Toolhelp), `ProcessWatch` (`OpenProcess` + `WaitForSingleObject`).
- `audio::capture`: process loopback игры с опросом PID раз в 2 с, переоткрытие через 3 с,
  системный loopback, событийный режим, 48 кГц / 2 канала / f32, буфер на 10 с,
  троттлинг лога переполнения.
- `audio::recorder`: WAV 48 кГц stereo f32, flush раз в 5 с, finalize, статистика.

## 3. Хвосты M0 (закрыть до крупных этапов)

Порядок — приоритет, каждый пункт = один коммит.

1. **AUD-4, пересэмплинг 16 кГц моно.** `audio::resample` на `rubato`:
   стерео суммировать и делить на 2 с ограничением ±1, затем 48 → 16 кГц.
   API: `struct Resampler48to16`, `fn push_stereo(&mut self, samples: &[f32]) -> Vec<f32>`;
   состояние не теряет сэмплы на границах блоков.
   Тест: синус 1 кГц 48 кГц → 16 кГц, проверить длину и частоту.
2. **Разбор каналов в `capture.rs`.** Сейчас на фрейм читается один f32
   (`sample_bytes = [0u8; 4]`), при 2 каналах второй сэмпл молча теряется.
   Читать `block_align` целиком, разбирать парами, суммировать по AUD-4.
   Тест — на чистой функции разбора байтов.
3. **AUD-6, `--file <WAV>`** через `symphonia`, тот же путь, что и захват,
   плюс `--no-overlay`, чтобы на файле отлаживать VAD/ASR.
4. **CLI-2, `--record` с путём в папку:** создавать родительскую папку, понятно
   ругаться, если файл занят.
5. **LOG-2, ротация 5 × 10 МБ** (`tracing_appender::rolling::Builder` с `max_log_files`).
6. **LOG-3, прогресс в лог:** каждые 10 с «Запись: N с, переполнений: M».
7. **NFR-7, MMCSS «Audio»** для потока захвата: `AvSetMmThreadCharacteristicsW("Audio")`,
   `AvRevertMmThreadCharacteristics` на выходе. Unsafe — только в модуле Win32.
8. **Кодировка лога.** `logs/tech.log.<дата>` пишется в OEM-кодировке, русский текст
   читается как «Р—Р°РїСѓСЃРє…». Писать UTF-8 без BOM.
9. **Тест троттлинга переполнения:** чистая функция
   `should_log_overflow(last: Instant, now: Instant) -> bool`.

Проверка после хвостов: команды из раздела 1 плюс
`scpsl-subs --record corpus\system\smoke.wav --duration 5` и
`scpsl-subs --file corpus\system\smoke.wav --no-overlay --debug`.

## 4. M1 — спайки: идём дальше или нет

### 4.1 Подключение ASR/VAD

Выбрать до кода:

- **Вариант A, рекомендую: готовый `sherpa-onnx` win64 shared.** Скачать релиз
  `sherpa-onnx-vX.Y.Z-win-x64-shared`, положить `sherpa-onnx-c-api.dll` и
  `onnxruntime.dll` в `vendor/sherpa/`, подключить через `build.rs` (`link-search` из
  `SHERPA_ONNX_DIR`). CMake и LLVM не нужны. Модели: `silero_vad_v5.onnx` (есть у нас,
  2,3 МБ) и `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`.
- **Вариант B: сборка `sherpa-rs` из исходников.** Нужны CMake 3.20+ и LLVM
  (libclang для `bindgen`). Дольше настраивать, зато всё из Cargo.

Распознаватель в любом случае за trait `Recognizer`, чтобы запасной путь работал без sherpa.

### 4.2 Задачи M1

1. `crates/voice`: `vad.rs` (Silero VAD v5), `segmenter.rs`, `asr.rs`
   (`trait Recognizer { fn recognize(&mut self, pcm16k: &[f32]) -> Result<Text> }`,
   реализация на Parakeet int8, 4 потока), `lang.rs`.
2. `crates/text`: `proxyapi.rs` (`HaikuClient::stream(req, on_delta)` на `ureq` с
   keep-alive; таймауты: connect 2 с, первый токен 3 с, весь ответ 8 с), `sse.rs`
   (построчный разбор `content_block_delta` / `message_delta`, включая `stop_reason: refusal`).
   Ключ из `PROXYAPI_KEY`, в лог не попадает никогда.
3. Проверить в ProxyAPI `cache_control` и `max_tokens: 0` (TXT-4, TXT-5).
4. Замеры: RTF для VAD и ASR, медиана до первого токена → `docs/m1_report.md`.
5. Запасной перевод: `text::fallback` (`trait Translator`) — Opus-MT через CTranslate2
   или candle, не второй ONNX Runtime. Спайк на одной фразе.
6. Порог решения: RTF ASR ≤ 0.3 и медиана до первого токена ≤ 1,5 с → M2.
   Иначе — подбор меньшей модели ASR до M2.

## 5. Дальше по этапам (детали в `PLAN_CODEX.md`)

- **M2:** нарезка фраз, промпт с глоссарием и контекстом (TXT-2), разбор
  `OK | DROP | CASSIE` с терпимостью к формату, оркестратор (2 параллельных запроса,
  очередь 4, порядок по номеру фразы, откат на локальный перевод после 3 сбоев,
  лимит в час), кэш, `UiEvent`, интеграционные тесты с `tools/fake_proxyapi`.
- **M3:** оверлей Direct2D/DirectWrite в layered-окне, DPI per-monitor v2, три фразы,
  затухание, F6-F10 через `RegisterHotKey`, трей, `vocab.txt`, crash-лог.
- **M4:** `--bench`, WER/RTF/задержки, JSON-отчёт, корпус 30-60 минут.
- **M5:** приёмка на Ryzen 7 4700U / 16 ГБ / Radeon, три сессии по 15 минут,
  трёхчасовой прогон, отключение сети, portable-архив, README с ценой и лицензиями.

## 6. Что нужно от пользователя

Блокирует прямо сейчас:

1. **Вход в GitHub CLI** — единственное действие, которое я выполнить не могу (нужен браузер):
   `& 'C:\Program Files\GitHub CLI\gh.exe' auth login --web --git-protocol https`
   Ввести 8-значный код, подтвердить. После этого создаю репозиторий и пушу.

Не блокирует (решения приняты, работаю дальше):

- **Ключ ProxyAPI** — пользователь впишет позже. До этого всё строится без ключа,
  проверка Haiku откладывается до момента, когда ключ появится.
- **Модели** — скачиваю сам в `vendor/`.
- **Доступ к `api.proxyapi.ru`** — проверить можно только после появления ключа;
  на M0-M1 не влияет.
- **Sherpa A/B** — решаю сам, начинаю с A (готовые DLL, без CMake/LLVM).
- **ТЗ в проект** (`docs/ТЗ.pdf` из Downloads) — удобнее с ним, но не блокирует:
  ID требований беру из `PLAN_CODEX.md`.

Не блокирует, но полезно:

- **Запись корпуса:** 10-15 минут голосового чата в `corpus/voice/`, 5 минут без голоса
  в `corpus/noise/`. Нужно для M4 и оценки WER.
- **Проверить имя процесса:** `Get-Process | Where-Object ProcessName -like '*scp*'`,
  по умолчанию в CLI стоит `SCPSL.exe`.

## 7. Порядок работ

1. ~~Закоммитить текущее состояние~~ — коммит `20facbf` (21.11.2026).
2. Вход в `gh`, `gh repo create`, первый `push` на GitHub.
3. Хвосты M0 (пункты 1-9 раздела 3) — коммит на каждый закрытый пункт.
4. M1 (раздел 4.2) и отчёт `docs/m1_report.md`.
5. Теги `m0-complete`, `m1-spikes` — точки возврата, чтобы смена решения A ↔ B
   не требовала ручного отката.

## История изменений

- 21.11.2026 — создан по итогам проверки M0 (сборка, тесты, живой захват).
- 21.11.2026 — зафиксированы ответы пользователя (раздел 0), сделан первый коммит.
