# Примеры

## Пайплайн с зависимостями

Зависимости выполняются первыми, каждая не более одного раза, поэтому порядок
такой: `clean` → `gen` → `build`.

```lua
task("clean", {
    desc = "удалить каталог сборки",
    run = function()
        if fs.exists("out") then fs.remove("out") end
    end,
})

task("gen", {
    desc = "сгенерировать исходники",
    deps = { "clean" },
    run = function()
        fs.mkdir("out")
        fs.write("out/version.txt", "1.0.0\n")
        print("gen: записал out/version.txt")
    end,
})

task("build", {
    desc = "собрать артефакт",
    deps = { "gen" },
    run = function()
        local version = fs.read("out/version.txt")
        fs.write("out/app.txt", "app " .. version)
        print("build: готово")
    end,
})
```

```sh
taku run build       # clean -> gen -> build
```

## Генерация задач в цикле

Задачи — это обычный Lua, поэтому их можно создавать из данных: по задаче на
модуль плюс агрегирующая задача, зависящая от всех (они идут параллельно).

```lua
local modules = { "core", "ui", "api" }

local stamps = {}
for _, name in ipairs(modules) do
    local task_name = "stamp-" .. name
    stamps[#stamps + 1] = task_name
    task(task_name, {
        desc = "отметить модуль " .. name,
        run = function()
            fs.mkdir("out")
            fs.write("out/" .. name .. ".stamp", "ok\n")
        end,
    })
end

task("stamp", {
    desc = "отметить все модули",
    deps = stamps,        -- { "stamp-core", "stamp-ui", "stamp-api" }
    run = function() print("отмечено модулей: " .. #stamps) end,
})
```

## Сведение: собрать много результатов в один

Несколько задач создают части; финальная ждёт их, затем сливает через `fs`.

```lua
local parts = { "header", "body", "footer" }

local part_tasks = {}
for i, part in ipairs(parts) do
    local name = "part-" .. part
    part_tasks[#part_tasks + 1] = name
    task(name, {
        run = function()
            fs.mkdir("parts")
            fs.write("parts/" .. i .. "-" .. part .. ".txt", part .. "\n")
        end,
    })
end

task("bundle", {
    desc = "склеить части по порядку",
    deps = part_tasks,
    run = function()
        local names = fs.read_dir("parts")
        table.sort(names)
        local out = ""
        for _, name in ipairs(names) do
            out = out .. fs.read("parts/" .. name)
        end
        fs.write("bundle.txt", out)
        print("bundle: частей " .. #names)
    end,
})
```

## Условные шаги через `env`

```lua
task("report", {
    run = function()
        local mode = env.get("MODE", "dev")     -- значение по умолчанию, если не задано
        print("сборка в режиме " .. mode)
        if mode == "release" then
            fs.mkdir("out")
            fs.write("out/RELEASE", "")
        end
    end,
})
```

```sh
taku run report                 # сборка в режиме dev
MODE=release taku run report    # ... и записывает out/RELEASE
```

Файл `.env` рядом с `Takufile.lua` загружается автоматически, поэтому `MODE`
можно держать в нём (реальная переменная окружения всё равно его переопределит):

```bash
# .env
MODE=release
```

Полный синтаксис см. в разделе [Автозагрузка `.env`](api/env#автозагрузка-env).

## Вызов ваших инструментов

Когда дойдёт до запуска реальных команд сборки/тестов, используйте `sh`. Команда —
это список аргументов (без оболочки); маленький помощник заваливает задачу при
ненулевом коде выхода:

```lua
local function run(argv)
    local code = sh.run(argv)
    if code ~= 0 then
        error(table.concat(argv, " ") .. " завершилась с кодом " .. code)
    end
end

-- Замените на команды, которые реально использует ваш проект.
task("test", { run = function() run({ "cargo", "test" }) end })
task("lint", { run = function() run({ "cargo", "clippy" }) end })

task("ci", {
    desc = "линт и тесты параллельно",
    deps = { "lint", "test" },
    run = function() print("ci: всё зелёное") end,
})
```

Для них нужны установленные программы — см. [sh](./api/sh.md): захват вывода,
опции `cwd`/`env`/`stdin` и пайплайны оболочки.
