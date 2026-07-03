# Changelog

All notable changes to Taku are documented here.

> **0.1.4 is a beta release.** Despite the clean `0.1.4` version (the Windows MSI
> installer requires a plain `X.Y.Z` version and rejects a `-beta` pre-release
> suffix), Taku is still beta-quality software — expect rough edges and breaking
> changes before 1.0.

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

> **0.1.4 — beta-релиз.** Несмотря на «чистую» версию `0.1.4` (Windows
> MSI-инсталлер требует версию вида `X.Y.Z` и не принимает предрелизный суффикс
> `-beta`), Taku по-прежнему beta-качества — возможны шероховатости и ломающие
> изменения до 1.0.

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
