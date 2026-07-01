# Установка

Выберите способ под свою платформу. Готовые бинарники публикуются в каждом
[релизе на GitHub](https://github.com/taidaru/taku/releases); для сборки из
исходников нужны Rust-тулчейн и компилятор C (Lua компилируется из исходников
через `mlua`).

## Linux и macOS (скрипт установки)

Самый быстрый способ — скачивает последний релиз под вашу ОС/архитектуру и
устанавливает бинарь `taku`:

```sh
curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
```

Устанавливает в `~/.local/bin` (или в `/usr/local/bin`, если он доступен на
запись и есть в `PATH`). Переопределяется переменными окружения:

```sh
# установить конкретную версию
TAKU_VERSION=v0.1.0-alpha.1 curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh

# установить в свой каталог
TAKU_INSTALL_DIR=~/bin curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
```

Если каталог установки не в `PATH`, скрипт подскажет, что добавить.

## Windows (Scoop)

Репозиторий Taku сам является бакетом [Scoop](https://scoop.sh):

```powershell
scoop bucket add taku https://github.com/taidaru/taku
scoop install taku
```

Обновление — `scoop update taku`. Чтобы поставить конкретную версию:

```powershell
scoop install taku@0.1.0-alpha.1
```

## Из архива релиза

Скачайте архив под свою платформу со
[страницы релизов](https://github.com/taidaru/taku/releases), распакуйте бинарь
`taku` и добавьте его в `PATH`:

- Linux: `taku-x86_64-unknown-linux-gnu.tar.gz`
- macOS (Apple silicon): `taku-aarch64-apple-darwin.tar.gz`
- Windows: `taku-x86_64-pc-windows-msvc.zip`

## Из исходников

При установленных Rust-тулчейне и компиляторе C:

```sh
cargo install --path crates/taku   # установит бинарь `taku`
# или
cargo build --release              # -> target/release/taku
```
