# Changelog

All notable changes to Taku are documented here.

> **0.1.5 is a beta release.** Despite the clean `0.1.5` version (the Windows MSI
> installer requires a plain `X.Y.Z` version and rejects a `-beta` pre-release
> suffix), Taku is still beta-quality software — expect rough edges and breaking
> changes before 1.0.

## [0.1.5] - 2026-07-20

The task body is no longer an imperative script — it's a declarative step
plan. This is a breaking change for any `Takufile.lua` written against 0.1.4.

### Added
- **Plan-based task model**: a task body is a list of *steps* (data) that Lua
  builds at load time and Rust executes — step constructors (`rm`, `cp`, `mv`,
  `write`, `append`, `mkdir`, `echo`, `confirm`, `argv`, `pipe`, `download`,
  `invoke`) replace ad-hoc imperative calls; `function(ctx) ... end` remains
  as an escape hatch with a live `ctx.vars`.
- Placeholder formatter: `${param}` / `$ENV_VAR` / `${$ENV_VAR}` / `$$`
  template resolution, plus `fmt()`/`raw()` builtins — a failed command
  prints its unresolved template, never a resolved secret.
- `unchanged { "glob", outputs = ... }` incremental guard: an xxHash64
  fingerprint over input file metadata, the step plan, vars, and the whole
  environment, stored per-task in `.taku/state/`; `--force` rebuilds,
  `--explain` reports what changed.
- `serve { ... }` for long-lived background services, with `ready = {
  timeout }` / `ready = { http, timeout }` readiness probes, exit-code
  monitoring, and group teardown on Ctrl+C or failure.
- `--dry-run` prints the dependency tree and each task's step plan without
  executing anything (templates stay unresolved, function-steps show as
  `<lua file:line>`).
- `--yes` auto-answers `confirm` steps; a run-scoped "done" set lets
  `invoke`d tasks satisfy later dependencies without rerunning.
- Load vs. runtime phase gating: effect APIs (`cmd.*`, `fs.*`, `net.*`) now
  error if called at load time — only step execution may perform effects.

### Changed
- Collapsed the local/remote backend split into free functions.
- Renamed `taku-shell` to `taku-cmd`: `cmd.run` raises on non-zero, `cmd.try`
  returns the exit code, `cmd.capture` returns `{code, stdout, stderr}`.
- Renamed several `fs`/`net` verbs for consistency; added `fs.glob` and
  sha256 verification to `net.download`.
- Rewrote the documentation site around the new step-plan model (English +
  Russian).

### Removed
- Dropped SSH support (`taku-ssh` and its tunnel plumbing) — out of scope
  for the plan-based core.

### Fixed
- `fs.rm` no longer errors when the target path doesn't already exist.
- Doc examples that used an invalid `cargo --profile debug` or an undeclared
  `${profile}` placeholder.

## [0.1.4] - 2026-07-03

First release distributed through a real installer pipeline. Highlights:

### Added
- New build & release pipeline based on **cargo-dist** — prebuilt binaries and
  installers are produced automatically for every release.

### Changed
- Greatly improved **Windows compatibility**.
- Refactored the API layer: shared registration plumbing extracted into a new
  `taku-api` crate (`RegisterCtx`, `ApiEntry`, the `lua_api!` macro) and a
  centralized error helper, so adding an API is a smaller, more auditable change.
- Richer, rustc-style error diagnostics: surrounding context lines around the
  error locus and rendered panic text.
- Expanded documentation site (English + Russian).

### Fixed
- `taku init` now writes a portable starter `Takufile.lua` on Windows.
- Miscellaneous fixes surfaced during the API refactor.

## [0.1.2-alpha] - 2026-07-01
- Initial public alpha.

---

# Изменения (RU)

> **0.1.5 — beta-релиз.** Несмотря на «чистую» версию `0.1.5` (Windows
> MSI-инсталлер требует версию вида `X.Y.Z` и не принимает предрелизный суффикс
> `-beta`), Taku по-прежнему beta-качества — возможны шероховатости и ломающие
> изменения до 1.0.

## [0.1.5] - 2026-07-20

Тело задачи больше не императивный скрипт — теперь это декларативный план
шагов. Это ломающее изменение для любого `Takufile.lua`, написанного под
0.1.4.

### Добавлено
- **Модель задач на основе плана**: тело задачи — список *шагов* (данные),
  которые Lua строит при загрузке, а Rust исполняет; конструкторы шагов
  (`rm`, `cp`, `mv`, `write`, `append`, `mkdir`, `echo`, `confirm`, `argv`,
  `pipe`, `download`, `invoke`) заменяют разрозненные императивные вызовы;
  `function(ctx) ... end` остаётся как escape hatch с живым `ctx.vars`.
- Форматтер плейсхолдеров: подстановка `${param}` / `$ENV_VAR` /
  `${$ENV_VAR}` / `$$`, а также билтины `fmt()`/`raw()` — упавшая команда
  печатает нерезолвленный шаблон, а не значения с секретами.
- Гвард инкрементальности `unchanged { "glob", outputs = ... }`: фингерпринт
  xxHash64 по метаданным входных файлов, плану шагов, переменным и всему
  окружению, хранится по задачам в `.taku/state/`; `--force` пересобирает,
  `--explain` объясняет, что изменилось.
- `serve { ... }` для долгоживущих фоновых сервисов: пробы готовности
  `ready = { timeout }` / `ready = { http, timeout }`, мониторинг кода
  выхода, групповое завершение по Ctrl+C или при ошибке.
- `--dry-run` печатает дерево зависимостей и план шагов каждой задачи, ничего
  не выполняя (шаблоны остаются нерезолвленными, function-шаги показаны как
  `<lua file:line>`).
- `--yes` автоматически отвечает на шаги `confirm`; набор «выполненных» задач
  в рамках запуска позволяет `invoke`-нутым задачам удовлетворять поздние
  зависимости без повторного запуска.
- Разделение фаз загрузки и выполнения: эффектные API (`cmd.*`, `fs.*`,
  `net.*`) теперь падают с ошибкой при вызове на этапе загрузки — эффекты
  доступны только во время исполнения шагов.

### Изменено
- Разделение local/remote backend схлопнуто в свободные функции.
- `taku-shell` переименован в `taku-cmd`: `cmd.run` кидает ошибку при
  ненулевом коде, `cmd.try` возвращает код выхода, `cmd.capture` возвращает
  `{code, stdout, stderr}`.
- Переименован ряд глаголов `fs`/`net` для единообразия; добавлены
  `fs.glob` и проверка sha256 в `net.download`.
- Документация переписана под новую модель плана шагов (английский +
  русский).

### Удалено
- Убрана поддержка SSH (`taku-ssh` и туннельная обвязка) — вне области
  ядра на основе плана.

### Исправлено
- `fs.rm` больше не падает с ошибкой, если целевого пути ещё не существует.
- Примеры в документации с невалидным `cargo --profile debug` и
  необъявленным плейсхолдером `${profile}`.

## [0.1.4] - 2026-07-03
Первый релиз с полноценным пайплайном установки.

### Добавлено
- Новый пайплайн сборки и релизов на базе **cargo-dist** — готовые бинарники и
  инсталлеры собираются автоматически для каждого релиза.


### Изменено
- Сильно улучшена **совместимость с Windows**.
- Рефакторинг API-слоя: общая инфраструктура регистрации вынесена в новый крейт
  `taku-api` (`RegisterCtx`, `ApiEntry`, макрос `lua_api!`) + централизованный
  error-helper — добавить новый API теперь проще и безопаснее для аудита.
- Более подробная диагностика ошибок в стиле rustc: контекстные строки вокруг
  места ошибки и отрисовка текста паники.
- Расширенная документация (английский + русский).

### Исправлено
- `taku init` пишет портируемый стартовый `Takufile.lua` на Windows.
- Мелкие исправления, выявленные при рефакторинге API.

## [0.1.2-alpha] - 2026-07-01
- Первая публичная alpha.
