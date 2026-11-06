# Character

AI-компаньон в «шторке» сверху экрана для рабочего стола COSMIC: следит за системой через D-Bus,
слушает обращения и выполняет ограниченный набор действий.
Подробнее — в [ARCHITECTURE.md](ARCHITECTURE.md), [CHARACTER.md](CHARACTER.md) и [DIAGRAMS.md](DIAGRAMS.md).

> **Статус: MVP-2 в работе.** Реализована layer-shell-шторка с ручными демонстрационными
> событиями, реакциями на состояние сети через NetworkManager и голосовым циклом:
> push-to-talk, локальная транскрипция через Ollama, диалог и опциональное локальное
> озвучивание. Уведомления и исполнение действий пока не подключены.

## Структура

```text
apps/
  character/          исполняемый файл (bin)
crates/               библиотеки (lib), по одной на компонент из ARCHITECTURE.md
packaging/appimage/   сборка AppImage (скрипт, .desktop, иконка)
.github/workflows/    CI и сборка AppImage
```

| Крейт | Роль (ARCHITECTURE.md, §2) | Состояние |
|---|---|---|
| `character-types` | общие типы: состояния FSM, эмоции, режимы приватности | есть |
| `character-core` | центральный демон: шина событий, FSM | правила переходов состояния сети |
| `character-actions` | allowlist-действия, уровни риска | манифест действия и уровни риска |
| `character-brain` | диалог и tool calling на [RIG](https://rig.rs/docs) | обёртка над агентом RIG, тесты на mock-модели |
| `character-ui` | шторка, рендер персонажа (libcosmic) | ручные события, реакции на сеть, push-to-talk и диалог |
| `character-watch` | наблюдатель D-Bus (zbus) | NetworkManager: состояние подключения |
| `character-voice` | локальный STT/TTS и аудиозапись | ALSA → Ollama audio API (Gemma 4), Piper/PipeWire |
| `character-memory` | память, настройки, профиль | каркас |

## Разработка

Версия Rust — `stable` (`rust-toolchain.toml`); минимальная версия задана в `rust-version` (`Cargo.toml`)
и определяется требованиями RIG.

Для UI нужны системные библиотеки и заголовки:

```sh
# Arch Linux
sudo pacman -S --needed base-devel cmake wayland libxkbcommon fontconfig freetype2 expat
# Ubuntu 24.04
sudo apt-get install pkg-config cmake libwayland-dev libxkbcommon-dev libfontconfig1-dev libfreetype-dev libexpat1-dev
```

libcosmic закреплён на git-ревизии в корневом `Cargo.toml`; транзитивные зависимости — в `Cargo.lock`.
Используется программный рендерер tiny-skia: для статичного MVP не требуется Vulkan.

```sh
cargo run -p character --locked                 # шторка в Wayland/COSMIC
cargo run -p character --locked -- --headless    # прежнее ядро-заглушка без графики
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

### MVP-0: управление

Нужна Wayland-сессия с `zwlr_layer_shell_v1` (например, COSMIC). Обычное окно и X11-fallback
не создаются. Шторка появляется по центру сверху на активном мониторе, учитывает место,
занятое системной панелью, и не резервирует рабочую область.

- Нажмите язычок **Fen**, чтобы развернуть шторку.
- **Новое**, **Готово**, **Отдых** переключают `curious`, `happy`, `sleepy` и соответствующий
  тултип. Это имитации: «успех» не запускает действие. Реальные реакции на сеть описаны ниже.
- **Свернуть** сворачивает шторку; **Выход** завершает приложение. Без взаимодействия шторка
  сворачивается через 12 секунд (с точностью таймера 250 мс). Пока указатель над ней,
  автосворачивание приостановлено; после ухода указателя отсчёт начинается заново.
- Клавиатурный фокус не захватывается. Микрофон выключен, что явно указано в UI.
  Для логов используйте `RUST_LOG=debug`; запуск из терминала можно завершить Ctrl+C / SIGTERM.

В MVP-0 состояние демонстрации локально для UI: IPC с ядром, приоритеты реальных событий,
fullscreen/DND и анимации относятся к следующим этапам. Размытие фона не требуется:
используется полупрозрачная панель и масштабируемые PNG-иллюстрации персонажа.

### MVP-1: сеть

`character-watch` подписывается на `StateChanged` NetworkManager и передаёт только
состояние «подключено/отключено»: имена сетей и другие свойства соединения не читаются.
Начальное состояние задаёт baseline без всплывающей реакции; эмоция и тултип появляются
только при смене состояния. Если NetworkManager или system D-Bus недоступны, ошибка
попадает в лог, а шторка и ручные демонстрационные события продолжают работать.

Мониторинг уведомлений не включён: стандартный `org.freedesktop.Notifications` не даёт
стороннему клиенту подписываться на входящие уведомления. Низкий заряд также не входит
в текущий объём MVP-1.

### MVP-2: push-to-talk

Голос по умолчанию выключен. Откройте шторку, нажмите **Enable voice**, затем удерживайте
**Hold to talk** во время речи и отпустите для отправки. Уход курсора за пределы
кнопки также завершает запись. Распознанный текст отправляется только локальному Ollama;
вызовы инструментов и системные действия не подключены. В текущем диалоге хранится не более
восьми последних ходов, только в памяти процесса.

Для голосового ввода нужны `arecord` (ALSA utilities) и запущенный локальный Ollama; по умолчанию
используется `gemma4:e4b`, поддерживающая аудио. Ollama получает WAV напрямую через
`/v1/audio/transcriptions`, отдельная модель Whisper не нужна. Для озвучивания дополнительно
нужны Piper CLI, модель Piper и `pw-play` (PipeWire). Настройки задаются переменными окружения:

```sh
export CHARACTER_PIPER_MODEL="$HOME/.local/share/character/en_US-lessac-medium.onnx" # optional
export CHARACTER_OLLAMA_MODEL="gemma4:e4b" # optional; must support audio input
export CHARACTER_OLLAMA_URL="http://localhost:11434" # optional
```

Файл WAV создаётся с правами владельца `0600` в системном временном каталоге, отправляется
на настроенный endpoint Ollama и удаляется после транскрипции или при отмене записи. При
переопределении `CHARACTER_OLLAMA_URL` аудио отправляется именно этому серверу. Текст разговора
не сохраняется на диск и очищается при выходе. Если `CHARACTER_PIPER_MODEL` не задан, ответ
остаётся доступен текстом без озвучивания. Исполняемые файлы и модели не включены в AppImage.

## LLM: RIG

Диалог строится на [RIG](https://rig.rs/docs). `character_brain::Brain` оборачивает агента RIG:

- `Brain::new(model, preamble)` — любая completion-модель RIG;
- `Brain::ollama(&OllamaConfig, model, preamble)` — локальный Ollama (по умолчанию `http://localhost:11434`).

Локальные модели — по умолчанию, облако — опционально (ARCHITECTURE.md, §1 и §7). Тестам LLM не нужен:
они используют mock-модель RIG. Голосовой путь использует Ollama из `character-ui`; инструменты
голосового ввода/вывода и модели остаются внешними локальными зависимостями и в AppImage не включаются.

## AppImage

### Сборка

```sh
packaging/appimage/build.sh   # → dist/Character-<версия>-x86_64.AppImage
```

Один и тот же скрипт работает локально и в CI. Нужны перечисленные выше зависимости UI,
`cargo`, `curl`, `sha256sum`, `file`;
если есть `desktop-file-validate`, проверяется `.desktop`. Инструменты упаковки (linuxdeploy, appimagetool, type2-runtime)
скачиваются в `target/appimage/tools`; их версии и sha256 закреплены в начале скрипта — чтобы обновить, замените URL и хеш.

### Запуск на Arch + COSMIC

```sh
chmod +x Character-*-x86_64.AppImage   # артефакты Actions теряют бит исполнения
./Character-*-x86_64.AppImage
```

- Runtime статический: `libfuse2` не нужен, но для монтирования образа нужен `fuse3` (`sudo pacman -S fuse3`).
  Без FUSE: `./Character-*-x86_64.AppImage --appimage-extract-and-run`.
- Образ собирается на ubuntu-24.04 (glibc 2.39), Arch — rolling и новее, поэтому совместимость сохраняется.
- Если запуск из терминала внутри другого AppImage-приложения стартует «не тот» образ — выполните `unset TARGET_APPIMAGE`.
- Если AppImageLauncher перехватывает запуск для интеграции в меню, для прямого запуска
  используйте `APPIMAGELAUNCHER_DISABLE=1 ./Character-*-x86_64.AppImage`.
- Иконка `packaging/appimage/character.svg` — временная заглушка.
- Wayland и графические драйверы берутся из системы, а не из AppImage.
  В COSMIC нужные runtime-библиотеки уже установлены.

### GitHub Actions

| Workflow | Когда | Что делает |
|---|---|---|
| `ci.yml` | push в `main`, PR | `cargo fmt`, `clippy -D warnings`, тесты |
| `appimage.yml` | push в `main`, PR, теги `v*`, вручную | собирает AppImage, проверяет `--version` / `--help` в `archlinux:latest` с runtime-библиотеками UI, публикует артефакт `Character-AppImage` |

Релиз: тег `vX.Y.Z` должен совпадать с версией в `Cargo.toml` (`[workspace.package]`).
Workflow приложит AppImage к GitHub Release.

```sh
git tag v0.1.0 && git push origin v0.1.0
```

Smoke-test CI проверяет запуск CLI, но не отрисовку: работу шторки нужно проверять
в Wayland/COSMIC, в том числе на реальном Arch + COSMIC.

## Что дальше

Порядок — по [дорожной карте](ARCHITECTURE.md#9-дорожная-карта). Следующие части MVP-2 —
wake-word и проверка аудиоцикла; MVP-1 продолжается подключением дополнительных источников,
если для них найдётся безопасный публичный API. Безопасные действия и allowlist относятся
к последующим этапам и пока не подключены.
Также позже: разделение на процессы с D-Bus-интерфейсом `org.character.Core1` и AppStream metainfo (после выбора app-id).

## Лицензия

[MIT](LICENSE)
