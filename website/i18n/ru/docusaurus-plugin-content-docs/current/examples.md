# Примеры

## Конвейер с зависимостями

Зависимости выполняются первыми, каждая не более одного раза, так что порядок
сборки: `clean` → `gen` → `build`.

```lua
--- удалить каталог сборки
task "clean" {
    rm "out",
}

--- сгенерировать исходники
task "gen: clean" {
    mkdir "out",
    write { "1.0.0", to = "out/version.txt" },
}

--- собрать артефакт
task "build: gen" {
    cp { "out/version.txt", to = "out/app.txt" },
    echo "build: готово",
}
```

```sh
taku run build       # clean -> gen -> build
```

## Инкрементальная сборка с параметрами

`unchanged` пропускает дорогую часть, когда ничего не изменилось; `--vars`
переопределяет параметр заголовка.

```lua
--- скомпилировать проект
task "build <profile=dev>" {
    unchanged { "src/**/*.rs", "Cargo.toml", outputs = "target" },
    "cargo build --profile ${profile}",
}
```

```sh
taku run build                        # полный прогон
taku run build                        # skip (unchanged)
taku run build --explain              # объяснит почему
taku run build --vars profile=release # vars входят в фингерпринт
```

## Dev-окружение с сервисами

`serve` держит долгоживущие процессы; как зависимости они поднимаются в фоне,
и граф продолжается после их готовности.

```lua
--- прогнать миграции базы
task "migrate" {
    "sqlx migrate run",
}

--- API-сервер
task "api: migrate" {
    serve {
        "cargo run -p api",
        ready = { http = "http://127.0.0.1:8000/health", timeout = 30 },
    },
}

--- веб-фронтенд
task "web" {
    serve { "npm run dev", cwd = "frontend" },
}

--- весь dev-стек, до Ctrl+C
task "dev: api web" {}
```

```sh
taku run dev
```

## Данные-шаги вперемешку с логикой

`function(ctx)`-шаг вычисляет значения для последующих плейсхолдеров;
`confirm` страхует разрушительный запуск.

```lua
--- опубликовать релиз
task "release <tag>" {
    confirm "опубликовать ${tag}?",
    function(ctx)
        local r = cmd.capture({ "git", "rev-parse", "--short", "HEAD" })
        ctx.vars.sha = r.stdout:gsub("%s+$", "")
    end,
    "git tag -a ${tag} -m 'релиз ${tag} (${sha})'",
    "git push origin ${tag}",
}
```

```sh
taku run release --vars tag=v1.2.0
taku run release --vars tag=v1.2.0 --dry-run   # предпросмотр: шаблоны не разрешаются
```

## Генерация тасков в цикле

Таски — это просто Lua, так что их можно определять из данных: по таску на
модуль плюс агрегатор, зависящий от всех (они идут параллельно).

```lua
local modules = { "core", "ui", "api" }

local names = {}
for _, name in ipairs(modules) do
    names[#names + 1] = "check-" .. name
    task("check-" .. name, {
        "cargo check -p " .. name,
    })
end

task("check-all: " .. table.concat(names, " "), {})
```

```sh
taku run check-all    # check-core, check-ui, check-api параллельно
```
