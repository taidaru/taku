# Установка

Выберите способ под свою платформу. Готовые бинарники и установщики
публикуются в каждом [релизе на GitHub](https://github.com/taidaru/taku/releases);
для сборки из исходников нужны Rust-тулчейн и компилятор C (Lua компилируется
из исходников через `mlua`).

## Linux и macOS (скрипт установки)

Самый быстрый способ — скачивает последний релиз под вашу ОС/архитектуру и
устанавливает бинарь `taku`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/latest/download/taku-installer.sh | sh
```

Устанавливает в `$CARGO_HOME/bin` (по умолчанию `~/.cargo/bin`) и обновляет
профиль шелла, чтобы бинарь был в `PATH`. Чтобы установить в свой каталог:

```sh
TAKU_INSTALL_DIR=~/bin curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/latest/download/taku-installer.sh | sh
```

Чтобы установить конкретную версию, возьмите установщик из соответствующего
релиза вместо `latest`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/download/v0.1.4/taku-installer.sh | sh
```

## Homebrew

Формулы публикуются в тапе [taidaru/homebrew-taku](https://github.com/taidaru/homebrew-taku):

```sh
brew install taidaru/taku/taku
```

## Windows (скрипт установки)

Скачивает последний релиз и устанавливает бинарь `taku`:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/taidaru/taku/releases/latest/download/taku-installer.ps1 | iex"
```

Для каждого релиза также доступен установщик `.msi` — на
[странице релизов](https://github.com/taidaru/taku/releases).

## Windows (Scoop)

Репозиторий [taidaru/homebrew-taku](https://github.com/taidaru/homebrew-taku) 
одновременно служит бакетом [Scoop](https://scoop.sh):

```powershell
scoop bucket add taku https://github.com/taidaru/homebrew-taku
scoop install taku
```

Обновление — `scoop update taku`. Чтобы поставить конкретную версию:

```powershell
scoop install taku@0.1.4
```

## Из архива релиза

Скачайте архив под свою платформу со
[страницы релизов](https://github.com/taidaru/taku/releases), распакуйте бинарь
`taku` и добавьте его в `PATH`:

- Linux: `taku-x86_64-unknown-linux-gnu.tar.xz` (есть также варианты `musl` и `aarch64`)
- macOS: `taku-aarch64-apple-darwin.tar.xz` (Apple silicon) или `taku-x86_64-apple-darwin.tar.xz`
- Windows: `taku-x86_64-pc-windows-msvc.zip`

Для каждого архива публикуется файл контрольной суммы `.sha256`. Tar-архивы
распаковываются в каталог `taku-<target>/`; zip-архивы — без каталога.

## Обновление

Чтобы обновить установку через скрипт установки, запустите ту же команду
установки повторно — она скачает и установит последний релиз поверх текущего.
Установки через Homebrew и Scoop обновляются пакетным менеджером как обычно.

## Из исходников

При установленных Rust-тулчейне и компиляторе C:

```sh
cargo install --path crates/taku   # установит бинарь `taku`
# или
cargo build --release              # -> target/release/taku
```
