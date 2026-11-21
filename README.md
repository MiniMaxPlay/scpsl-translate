# scpsl-subs

Начальная реализация M0: запись звука в WAV 48 кГц stereo f32 через WASAPI.

## Подготовка Windows

Установить Rust stable с target `x86_64-pc-windows-msvc` и Visual Studio Build Tools 2022 с компонентом C++.

## Сборка

```text
cargo build --release
```

## Запись

Игра должна работать в режиме «окно без рамки».

```text
scpsl-subs --record corpus\voice\session.wav
scpsl-subs --record corpus\voice\session.wav --duration 900
scpsl-subs --record corpus\system.wav --source system
```

Для `game` используется process loopback `SCPSL.exe`; если процесс не запущен, программа ждёт его и подключается автоматически. Для отладки можно изменить имя через `--game-process`.

Корпус рекомендуется хранить так: `corpus/voice/*.wav`, `corpus/noise/*.wav`, `corpus/stream/*.wav`, `corpus/labels/*.srt`.