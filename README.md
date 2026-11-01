# Character

AI-компаньон в «шторке» сверху экрана для рабочего стола COSMIC: следит за системой через D-Bus,
слушает обращения и выполняет ограниченный набор действий.
Подробнее — в [ARCHITECTURE.md](ARCHITECTURE.md), [CHARACTER.md](CHARACTER.md) и [DIAGRAMS.md](DIAGRAMS.md).

> **Статус: каркас проекта.** Готовы workspace, доменные типы, интеграция с RIG, CI и сборка AppImage.
> Шторка (MVP-0) ещё не реализована: приложение пока запускает ядро-заглушку.

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
| `character-core` | центральный демон: шина событий, FSM | заглушка: стартует и ждёт сигнал завершения |
| `character-actions` | allowlist-действия, уровни риска | манифест действия и уровни риска |
| `character-brain` | диалог и tool calling на [RIG](https://rig.rs/docs) | обёртка над агентом RIG, тесты на mock-модели |
| `character-ui` | шторка, рендер персонажа (libcosmic) | каркас |
| `character-watch` | наблюдатель D-Bus (zbus) | каркас |
| `character-voice` | wake-word, STT, TTS | каркас |
| `character-memory` | память, настройки, профиль | каркас |

## Разработка

Версия Rust — `stable` (`rust-toolchain.toml`); минимальная версия задана в `rust-version` (`Cargo.toml`)
и определяется требованиями RIG.

```sh
cargo run -p character                                        # RUST_LOG=debug для подробных логов; выход — Ctrl+C / SIGTERM
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

## LLM: RIG

Диалог строится на [RIG](https://rig.rs/docs). `character_brain::Brain` оборачивает агента RIG:

- `Brain::new(model, preamble)` — любая completion-модель RIG;
- `Brain::ollama(&OllamaConfig, model, preamble)` — локальный Ollama (по умолчанию `http://localhost:11434`).

Локальные модели — по умолчанию, облако — опционально (ARCHITECTURE.md, §1 и §7). Тестам LLM не нужен:
они используют mock-модель RIG. `character-core` пока не зависит от `character-brain`, поэтому RIG в AppImage ещё не попадает.

## AppImage

### Сборка

```sh
packaging/appimage/build.sh   # → dist/Character-<версия>-x86_64.AppImage
```

Один и тот же скрипт работает локально и в CI. Нужны `cargo`, `curl`, `sha256sum`, `file`;
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
- Иконка `packaging/appimage/character.svg` — временная заглушка.

### GitHub Actions

| Workflow | Когда | Что делает |
|---|---|---|
| `ci.yml` | push в `main`, PR | `cargo fmt`, `clippy -D warnings`, тесты |
| `appimage.yml` | push в `main`, PR, теги `v*`, вручную | собирает AppImage, проверяет его в чистом контейнере `archlinux:latest`, публикует артефакт `Character-AppImage` |

Релиз: тег `vX.Y.Z` должен совпадать с версией в `Cargo.toml` (`[workspace.package]`).
Workflow приложит AppImage к GitHub Release.

```sh
git tag v0.1.0 && git push origin v0.1.0
```

COSMIC в CI не запустить, поэтому запуск на реальном Arch + COSMIC проверяется вручную.

## Что дальше

Порядок — по [дорожной карте](ARCHITECTURE.md#9-дорожная-карта). Ближайший шаг — MVP-0: шторка на libcosmic в `character-ui`.
libcosmic подключается как git-зависимость; для сборки в CI понадобятся системные dev-пакеты, а графические библиотеки
(Wayland, GPU) в AppImage не упаковываются — берутся из системы.
Также позже: разделение на процессы с D-Bus-интерфейсом `org.character.Core1` и AppStream metainfo (после выбора app-id).

## Лицензия

[MIT](LICENSE)
