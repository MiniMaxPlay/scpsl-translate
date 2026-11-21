__Цель:__ написать на Rust программу `scpsl-subs`. Она берёт звук процесса SCPSL.exe, определяет речь (Silero VAD), распознаёт её (Parakeet через sherpa-onnx), проверяет и переводит текст через Haiku 5.5 по ProxyAPI с запасным переводом Opus-MT и показывает субтитры в прозрачном окне поверх игры. Источник требований — ТЗ (PDF в Downloads); ID требований (AUD-1, TXT-3…) указываются в коммитах и тестах.

__Где что лежит:__ прототип на Python в `C:\Users\maksi\Documents\scp sl translater mod`, его только читаем. Новая программа — в `C:\Users\maksi\Documents\scpsl translate`.

### Файлы, которые нужно создать

```javascript
Cargo.toml                      # workspace, общие версии зависимостей
crates/app/      main.rs, cli.rs, pipeline.rs, tray.rs, hotkeys.rs
crates/audio/    capture.rs (process loopback), resample.rs, record.rs, file.rs
crates/voice/    vad.rs, segmenter.rs, asr.rs (trait Recognizer), lang.rs
crates/text/     filter.rs, normalize.rs, proxyapi.rs, sse.rs, verdict.rs,
                 fallback.rs (trait Translator), cache.rs, prompt.rs
crates/overlay/  window.rs, render.rs (D2D/DWrite), layout.rs, model.rs
crates/config/   config.rs (toml_edit), glossary.rs, state.rs, default_config.toml
crates/bench/    corpus.rs, wer.rs, report.rs
assets/glossary.toml, assets/system_prompt.txt
tools/fake_proxyapi/            # локальная заглушка ProxyAPI для интеграционных тестов
README.md
```

### Правила для Codex

- Код без `unsafe`, кроме модулей Win32 и FFI. Должны проходить `cargo clippy -- -D warnings` и `cargo fmt --check`.
- Сообщения пользователю и лог — на русском, имена в коде — на английском.
- В процессе одна копия ONNX Runtime, из sherpa-onnx. Запасной перевод — CTranslate2 или candle, не второй ORT.
- Ключ API никогда не попадает в лог. Не внедряться в игру и не ставить хуков (NFR-1).
- Этап закрывается только после проверочных команд из его раздела.

### Шаги

__Шаг 0 (делает пользователь).__ Установить Rust stable (MSVC), VS Build Tools 2022 с C++, CMake и LLVM, задать `PROXYAPI_KEY`.

__M0 — каркас и запись звука__

1. Workspace на 7 крейтах, `clap` CLI с флагами из CLI-1 и CLI-2, логи через `tracing` + `tracing-appender` с ротацией 5×10 МБ (LOG-2).
2. `config`: `Config::load_or_create(path) -> (Config, Vec<Warning>)`. При первом запуске пишет файл с русскими комментариями, неверное значение заменяет значением по умолчанию с предупреждением (CFG-1). Плюс `State` с позицией окна в долях монитора (CFG-2) и `Glossary::load`.
3. `audio::capture`: `GameCapture::start(process: &str, tx: rtrb::Producer<f32>)`. Захват через WASAPI process loopback, пока игры нет — проверка процесса раз в 2 с, при обрыве — переоткрытие через 3 с (AUD-1…AUD-3). Поток захвата получает приоритет MMCSS «Audio».
4. `audio::resample`: перевод в 16 кГц моно через `rubato`, каналы суммируются и делятся на 2 с ограничением ±1 (AUD-4).
5. `--record out.wav`: запись звука игры в WAV 48 кГц (AUD-5). `--file` через `symphonia` (AUD-6), `--source system` (AUD-8).
6. __Проверка:__ `cargo test -p config -p audio`; `scpsl-subs --record test.wav` при запущенной игре.

__M1 — спайки и замеры (решение «идём дальше или нет»)__

1. `voice`: `sherpa-rs` или свои биндинги к C API. Проверить Silero VAD v5 и Parakeet v3 int8 на 4 потоках и записать RTF.
2. `text::proxyapi`: `HaikuClient::stream(req, on_delta) -> Result<TextVerdict>` на `ureq` с keep-alive. Таймауты: подключение 2 с, первый токен 3 с, весь ответ 8 с.
3. `text::sse`: построчный разбор событий `content_block_delta` и `message_delta`, включая `stop_reason: refusal`.
4. Проверить у ProxyAPI `cache_control` и `max_tokens: 0`. Записать в `docs/m1_report.md` медиану до первого токена.
5. Запасной перевод: собрать `ct2rs` с Opus-MT en→ru int8, если не выходит — candle с моделью Marian. Записать задержку.
6. __Условие перехода:__ если медиана до первого токена больше 1 с или распознавание дольше 350 мс, остановиться и пересмотреть цели раздела 3 вместе с пользователем.

__M2 — конвейер (5 потоков)__

1. `voice::segmenter`: машина состояний по VAD-1…VAD-6. Вход — вероятности кадров по 512 сэмплов, выход — `SegEvent::{Partial, End}`. Конец фразы через 250 мс тишины, если черновик кончается на «.», «?» или «!», иначе через 600 мс; мягкий разрез на 6 с, жёсткий на 8 с с пометкой `cont`.
2. `voice::asr`: `trait Recognizer { fn transcribe(&mut self, &[f32]) -> Result<AsrResult> }`. Финальные фразы — очередь на 4, черновик — один слот, финалы идут первыми (ASR-4). Язык определяет `whatlang`: русскую речь показываем сразу без перевода (ASR-3).
3. `text::filter` и `normalize`: пустые и шумовые фразы, обрезка повторов больше 3 подряд, таблица замен терминов (ASR-5, ASR-6).
4. `text::prompt`: кэшируемый system-промпт (правила, глоссарий, примеры) и сообщение с контекстом из последних фраз (TXT-2).
5. `text::verdict`: разбор ответа `OK | DROP | CASSIE` + `RU:` + необязательная `EN:`. Разбор терпит отклонения от формата (TXT-3).
6. Оркестратор перевода: до 2 запросов параллельно, очередь на 4, порядок на экране по номеру фразы (TXT-9). После 3 сбоев подряд — 60 с только локальный перевод (TXT-10). Плюс LRU-кэш, словарь коротких реплик, лимит запросов в час и тосты об ошибках 401/402/429 (TXT-11…TXT-14).
7. Главный поток получает события `UiEvent` (типы как в разделе 7 ТЗ) через `crossbeam-channel` и `PostMessage(WM_APP)`.
8. __Проверка:__ `cargo test --workspace` (нарезка на синтетических данных, разбор SSE и ответа, кэш); `scpsl-subs --file corpus\x.wav --fast --no-overlay --translator haiku`; интеграционные тесты с заглушкой `fake_proxyapi` (задержка, обрыв, 401, 402, 429).

__M3 — оверлей и управление__

1. `overlay::window`: layered-окно со стилями из OVL-1, поддержка DPI по Per-monitor v2.
2. `render`: Direct2D + DirectWrite, текст с обводкой по контуру глифов, вывод через `UpdateLayeredWindow` (OVL-2). Перерисовка только по событию, таймер работает только пока что-то тускнеет (OVL-13).
3. `layout` и `model`: блок по центру внизу экрана, максимум три фразы, перенос по словам, оригинал и перевод, серый черновик, C.A.S.S.I.E. и затухание (OVL-3…OVL-11).
4. Режим перемещения на F6 с сохранением позиции; возврат поверх всех окон при смене активного окна через `SetWinEventHook` и страховочный таймер 2 секунды (OVL-12, OVL-14).
5. Реализовать F6/F7/F8/F9/F10 через `RegisterHotKey`: перемещение, показать/скрыть, перевод, сохранение в `vocab.txt`, пауза. Занятая клавиша выдаёт тост и запись в лог (CTL-1, CTL-2, CTL-4).
6. Реализовать tray-меню: показать/скрыть, пауза, настройки, папка логов, выход. Консоль показывать только при `--console` (CTL-3).
7. Сохранять сессионный лог, последнюю фразу в `vocab.txt`, crash-лог с трассировкой; паника рабочего потока не должна завершать программу (LOG-1…LOG-4, NFR-5).

Проверка M3: три игровые сессии по 15 минут; оверлей не забирает фокус, клики проходят сквозь него, работают хоткеи, трей и сохранение позиции.

## Этап M4 — benchmark и тестовый корпус

1. Реализовать `--bench <папка>` с WAV и JSON/SRT-разметкой.
2. Собирать WER, RTF, p50/p95/max задержки до распознанного текста, первого токена и полного перевода.
3. Собирать CPU, RAM, FPS, размеры очередей, количество DROP/ложных фраз, токены и стоимость фразы.
4. Формировать JSON-отчёт и русскую сводку; тот же корпус прогонять через Python-прототип для сравнения.
5. Проверить корпус: 30–60 минут голосового чата, 10 минут ручной разметки, 30 минут игрового звука без голоса, 10 минут непрерывной речи и 100 фраз с эталонным переводом.

## Этап M5 — приёмка и поставка

1. Выполнить PERF-1…PERF-6, RES-1…RES-5 и Q-1…Q-4 на эталонном ноутбуке Ryzen 7 4700U, 16 ГБ RAM, Radeon iGPU.
2. Выполнить три игровые сессии по 15 минут и трёхчасовой непрерывный прогон.
3. Отключить сеть во время игры: локальный перевод должен продолжать работать; после возврата сети снова использовать Haiku.
4. Собрать portable-архив без Python и установщика: exe, DLL, models, `config.toml`, `glossary.toml`, README.
5. Проверить Windows 10 build 20348+ и Windows 11, масштаб 100–200% и несколько мониторов.
6. В README описать установку, `PROXYAPI_KEY`, клавиши, режим «окно без рамки», частые проблемы, стоимость и лицензии моделей/библиотек.

## Финальная проверка

```text
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo build --release
```

При провале сначала исправлять первопричину и затем повторять полный набор проверок. Не оставлять `TODO`, пустые функции или заглушки в релизном коде.