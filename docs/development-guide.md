# DigUp Plus: руководство разработчика (Windows-форк)

**Дата:** 2026-10-27

**Статус:** актуально для ветки `file-operations` (форк `Sucotasch/DigUpPlus`, upstream `ARahim3/DigUp`).

**Зачем этот файл:** чтобы человек или агент, открывший репозиторий без контекста, мог собрать, протестировать и
упаковать проект на этой машине, не выясняя заново пути, флаги и причины прошлых решений. Здесь только то, что
не очевидно из кода и `CONTRIBUTING.md`.

Сначала прочитайте `CONTRIBUTING.md` (правила для upstream) и `README.md` («Build from source»). Этот файл — про
конкретную Windows-машину и про fork-specific работу.

## 1. Что это за репозиторий

DigUp Plus — локальный семантический поиск по своим файлам. Движок — Swift (`Sources/DigUpKit`), приложение — Tauri
(`shell/`, Rust + обычные HTML/CSS/JS без сборки). Модель — EmbeddingGemma 2 Q8_0 на llama.cpp, всё считается на
машине, сети нет (кроме одноразовой загрузки модели).

macOS-приложение (`Sources/DigUpApp`) в дереве осталось как эталон поведения, но в форке **не собирается**: в
`Package.swift` для него нет ни продукта, ни зависимости на Sparkle, `build.sh` и `scripts/release.sh` удалены.
Форк ориентирован на эту Windows-машину; portable-сборка — основной формат, установщик остался как запасной.

Этот checkout — **Windows-форк**. `origin` = `https://github.com/Sucotasch/DigUpPlus.git`,
`upstream` = `https://github.com/ARahim3/DigUp.git`. В форке живут:

- пагинация выдачи в главном окне и слои Images (см. `docs/search-results-pagination-plan.md`);
- файловые операции в главном окне: Move To / Copy To / Delete (см. `docs/file-operations-plan.md`);
- настройки папки индекса и модели, установщик Windows.

`upstream` не пушить. Автообновлений в форке нет: модуль `updates.rs`, плагин updater и настройка «Check for
updates automatically» удалены, релизы выпускаются вручную. Вместо проверки обновлений Settings → General
показывает версию и кнопку Download, которая открывает страницу релизов форка (`open_releases`).

## 2. Раскладка

| Путь | Что внутри |
|---|---|
| `Sources/DigUpKit` | Движок: `Crawler`, экстракторы, `IndexStore`/`Database` (SQLite + FTS5), `Searcher`, `Indexer`, протокол `Worker`. Без UI. |
| `Sources/DigUpKit/Platform/Apple` | macOS: ImageIO, PDFKit, AVFoundation, Vision, Metal. |
| `Sources/DigUpKit/Platform/Portable` | Windows/Linux: PDFium, stb, miniz, SQLite, ffmpeg из `Vendor/`. |
| `Sources/LlamaRuntime` | Биндинг llama.cpp и инференс EmbeddingGemma. |
| `Sources/digup` | CLI разработчика **и** helper-процессы приложения: `index-service`, `search-service`, `worker`, `download-model`, `estimate`, `search`, `status`, `speed`, `eval`. |
| `Sources/DigUpApp` | macOS-приложение (эталон поведения). В форке не собирается ни на одной платформе. |
| `shell/src-tauri` | Rust-оболочка: трей, хоткей, окна, файловые операции, статус. |
| `shell/ui` | Окна оболочки: `panel.*`, `window.*`, `settings.*`, `setup.*`. Чистые HTML/CSS/JS, без фреймворка и сборки. |
| `Tests/DigUpKitTests`, `Tests/LlamaRuntimeTests` | Swift Testing (`@Suite`/`@Test`/`#expect`). |
| `evals/*.json` | Наборы запросов с известными ответами для `digup eval`. |
| `scripts/` | Сборка llama.cpp/ffmpeg, vendor, упаковка, dev-run, testbed. |
| `Vendor/` | Генерируется `scripts/fetch-vendor.sh`, в git не хранится. |
| `.build/`, `.dev/` | Локальные артефакты, индексы, модели и одноразовые стенды. В git не хранятся. |

`AGENTS.md` и `knowledge.md` в корне — локальные заметки для агентов; они в `.gitignore`, в коммит не попадают.
Этот файл — их публичный, коммитируемый аналог.

## 3. Окружение этой машины

| Что | Где / версия |
|---|---|
| ОС | Windows 10, i7-5820K (6/12), RTX 4070 Ti SUPER 16 ГБ, CUDA 13.1 |
| Репозиторий | `D:\Arx\Software Downloads\_Images_EDIT-pack\DigUp` (в пути есть пробелы — всегда в кавычках) |
| Git Bash | `D:\Works\Git\bin\bash.exe` |
| Swift | 6.4, `C:\Users\sucot\AppData\Local\Programs\Swift\Toolchains\6.4.0+Asserts\usr\bin\swift.exe`; runtime DLL — `...\Runtimes\6.4.0\usr\bin` |
| Visual Studio | 2022 Community; `vcvars64.bat` в `...\VC\Auxiliary\Build\`; MSVC tools 14.44; CRT redist `...\VC\Redist\MSVC\14.44.35112\x64\Microsoft.VC143.CRT` |
| Rust | `C:\Users\sucot\.cargo\bin`; shell линкует C runtime статически (`shell/src-tauri/.cargo/config.toml`) |
| Node | `C:\Program Files\nodejs`; npm CLI — `C:\Program Files\nodejs\node_modules\npm\bin\npx-cli.js` |
| Python | `python` (3.x). `python3` — заглушка Microsoft Store, не работает |
| Модель | `.dev/models`: `embeddinggemma-2-Q8_0.gguf` (310 МБ) + `mmproj-...` (555 МБ) |
| Живой индекс | `.build/asian-index` — 3 772 изображения, на нём проверялись пагинация и слои |
| Chrome for Testing 137 | `C:/Users/sucot/.ext-dev-loop/browsers/cft-137/chrome-win64/chrome.exe` (вне репозитория) |

Главное:

- **Голый `bash` на этой машине — это WSL** (`C:\Windows\System32\bash.exe`), а не Git Bash. Он выберет
  Linux-бинарники из `Vendor/` и неверные пути. Все shell-скрипты запускайте через
  `D:\Works\Git\bin\bash.exe` (или из Git Bash).
- `.build/` — свалка локальных стендов и логов; она не коммитится. Скрипты оттуда (`package-ui.sh`,
  `build-ui-installer.cmd`, CDP-стенды) существуют только на этой машине; их рецепты продублированы ниже,
  чтобы не потерять.
- `Vendor/` и `shell/src-tauri/helper/` тоже не коммитятся: первое собирается `scripts/fetch-vendor.sh`,
  второе — упаковкой.

## 4. Swift: сборка, тесты, CLI

### Сборка

```sh
swift build --product digup                 # из PowerShell тоже работает
python .build/swift_with_vc.py build -c release   # тонкая обёртка: добавляет DLL-папки SwiftPM/Runtime, печатает EXIT
```

- Бинарник: `.build/out/Products/Release-windows-x86_64/digup.exe` (путь надёжнее, чем `.build/release`).
- SwiftPM 6.4 не может создать symlink `.build\release` на Windows (I/O error 512). Это **безвредно**, сборка
  продолжается. Не «чините» это.

### Тесты

```sh
python .build/swift_test.py                        # всё
python .build/swift_test.py --filter SearchPagination
python .build/swift_test.py --filter "DigUpKitTests"
```

Почему не `swift test` напрямую: без окружения он собирает, а потом рапортует все таргеты как failed без вывода.
`swift_test.py` добавляет:

- окружение `vcvars64.bat`;
- DLL-папки Swift Testing/XCTest;
- `Vendor/pdfium/bin`, `Vendor/llama/lib`, `Vendor/ffmpeg/bin`, CUDA 13.1;
- debug CRT из Visual Studio;
- `DIGUP_BACKENDS=Vendor/llama/lib`.

Известный локальный сбой: `IndexStoreTests.modelFilesComeFromTheHuggingFaceCacheWhenThere` падает с Win32 1314 —
тест создаёт symlink, а Developer Mode выключен. Это ограничение среды, не регрессия; тест не менять. Полный
прогон (2026-10-27) — 133 из 134, единственный сбой этот.

`SearchPaginationTests` полностью офлайновые: строят временный индекс в тесте, модель не нужна, идут ~1,5 с.
Флаг `--filter` принимает имя сюиты или теста.

### CLI на живом индексе

```sh
DIGUP_MODELS="$PWD/.dev/models" DIGUP_BACKENDS="$PWD/Vendor/llama/lib" \
PATH="C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v13.1/bin/x64:$PATH" \
.build/out/Products/Release-windows-x86_64/digup.exe search "asian" \
  --index .build/asian-index -n 120 --json
```

`--zgate 0` снимает смысловой порог (удобно для замеров пагинации). `DIGUP_MODELS` нужен для эмбеддинга запроса;
без модели CLI работает в keyword-режиме. `digup --help` — остальные команды.

## 5. Shell (Tauri): сборка, тесты, живой запуск

```sh
cd shell/src-tauri
cargo test --locked      # 56 тестов
cargo build --locked
```

Если cargo в Git Bash берёт не тот линкер (`/usr/bin/link.exe` затмевает MSVC), задайте:

```sh
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe"
export PATH="$(dirname "$CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER"):$PATH"
```

**UI вкомпилируется в бинарник.** После любой правки `shell/ui/*` нужен `cargo build` или полная упаковка,
иначе окно останется старым. Проверка синтаксиса:

```sh
for f in shell/ui/*.js; do node --check "$f" || echo "FAIL $f"; done
```

Живой запуск с черновыми индексом/настройками, чтобы не трогать реальные данные:

```powershell
$bin = swift build --show-bin-path
shell\src-tauri\target\debug\digup-shell.exe --helper "$bin\digup.exe" `
  --index .build\dev\index --roots "<папка>" --settings .build\dev\settings.json `
  --models .dev\models
```

- `--debug-window "" --debug-show` — открыть большое окно;
- `--debug-window "<запрос>" --debug-quit` — напечатать отчёт страницы и выйти;
- `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` перед запуском — управлять окном по CDP.

Перед повторным запуском закройте предыдущий экземпляр: окно/трей/индекс-сервис могут держать файлы.

## 6. Сборка для Windows

### Portable — то, что форк выпускает (`scripts/package-portable.sh`)

```sh
scripts/package-portable.sh        # → shell/src-tauri/target/release/DigUpPlus.exe
```

Скрипт собирает helper-папку так же, как установщик, затем `npx @tauri-apps/cli@2.12.2 build --no-bundle` и кладёт
результат в `shell/src-tauri/target/release` — единственное место сборки: `DigUpPlus.exe` рядом с `helper/`,
`THIRD-PARTY-NOTICES.txt` и `portable.txt` (старые имена exe убираются). Ничего не устанавливается: папку можно
скопировать куда угодно и запустить. Благодаря `portable.txt`
оболочка держит настройки, логи, индекс, модель и профиль WebView2 в папке `data` рядом с exe (см. §5), а не в
`%APPDATA%`/`%LOCALAPPDATA%`; `--data <папка>` или `DIGUP_DATA` указывают другое место. Без `portable.txt` (установщик)
всё по-прежнему в `%APPDATA%\com.sucotasch.digupplus` и `%LOCALAPPDATA%\DigUpPlus`.

### Установщик (остался от upstream, `scripts/package-windows.sh`)

`scripts/package-windows.sh` — полная упаковка. Требования (заголовок скрипта): Git Bash внутри Visual Studio
developer environment, `scripts/fetch-vendor.sh`, `scripts/build-llama.sh`, `scripts/build-ffmpeg.sh` (последний —
в MSYS2 MINGW64), Swift, Rust, Python, Node.

Внутри: `swift build -c release --product digup` → сбор helper-папки (`digup.exe`, DLL llama.cpp, PDFium, CUDA
runtime, Swift runtime, C++ CRT, ffmpeg/ffprobe) → `scripts/notices.py` → `npx @tauri-apps/cli@2.12.2 build
--bundles nsis` с `helper/` и `THIRD-PARTY-NOTICES.txt` как resources.

Выход: `shell/src-tauri/target/release/bundle/nsis/DigUp Plus_<version>_x64-setup.exe` (~440 МБ).
Версия берётся из `shell/src-tauri/tauri.conf.json` (сейчас `0.1.1`), не из `Cargo.toml` (там `0.1.0` у пакета
оболочки, но имя файла и настройки идут от `tauri.conf.json`). Подписи и update-артефактов больше нет: `signing` из
скрипта убран вместе с плагином updater.

### Быстрый путь: изменился только UI

Когда Swift-helper уже собран и менялись только `shell/ui/*`, полный `swift build` не нужен. На этой машине для
этого есть локальные обёртки (в git их нет):

- `.build/package-ui.sh` — берёт готовый `.build/out/Products/Release-windows-x86_64/digup.exe`, собирает
  helper-папку, генерирует notices и запускает Tauri CLI. Версии Swift/CRT/CUDA захардкожены — обновляйте при
  обновлении Visual Studio.
- `.build/build-ui-installer.cmd` — вызывает `vcvars64.bat`, выставляет MSVC-линкер и PATH (Swift, cargo, node,
  Git Bash) и запускает `package-ui.sh`.
- `.build/package-local.sh` + `.build/build-installer.cmd` — то же самое, но с полной сборкой helper'а
  (копия `scripts/package-windows.sh` с путями этой машины).

Запуск из корня репозитория:

```sh
MSYS_NO_PATHCONV=1 cmd.exe /c ".build\\build-ui-installer.cmd"
```

`MSYS_NO_PATHCONV=1` обязателен, иначе Git Bash превратит `/c` в путь. Команда идёт ~20–25 минут (cargo release +
NSIS; сам NSIS — последние ~4 минуты). Успех виден по `PACKAGE_EXIT 0` и свежему установщику.

Проверка результата:

```sh
ls -la shell/src-tauri/target/release/digup-shell.exe dist/
md5sum shell/src-tauri/target/release/bundle/nsis/*_x64-setup.exe dist/*.zip   # если сравниваете сборки
```

### Грабли упаковки

| Симптом | Причина / решение |
|---|---|
| `link.exe` не тот, странные ошибки линковки | Git Bash подсунул `/usr/bin/link.exe`; задайте `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` и PATH (см. §5) |
| `npx: command not found` | В окружении vcvars нет npm-шима; вызывайте `node "C:/Program Files/nodejs/node_modules/npm/bin/npx-cli.js" --yes @tauri-apps/cli@2.12.2` |
| `python3` открывает Store / падает | `python3` — заглушка; скрипты уже падают обратно на `python`, но не вызывайте `python3` вручную |
| Скрипт требует CUDA | `ggml-cuda.dll` нужен CUDA runtime; без него helper молча уйдёт на CPU. Задайте `CUDA_PATH` |
| В `bundle/nsis` старый установщик другой версии | Скрипты сами делают `rm -rf` папки; если запускаете Tauri CLI вручную — удалите её |
| Установщик собрался, но UI старый | Страницы вкомпилированы; нужен `cargo build`/Tauri build после правки `shell/ui` |
| `package-ui.sh` взял старый helper | Он не собирает Swift; сначала `swift build -c release --product digup` или полный `package-windows.sh` |
| Подпись/обновления | Ни подписи, ни автообновления: релизы вручную, zip заменяет папку |

## 7. Проверка UI без установки

Окно можно прогнать headless на настоящих `window.html/css/js` с замоканным Tauri-мостом. Стенд этого форка:

```sh
node .build/make-tier-harness.cjs     # собирает .build/tier-bench/bench.html из shell/ui + calibration-hits.json
node .build/run-tier-cdp.cjs          # обычные пороги
TIER_LOW=1 node .build/run-tier-cdp.cjs   # защита: вся шкала cosine ниже обычных порогов
```

Что делает `run-tier-cdp.cjs`: запускает Chrome for Testing 137 headless с `--remote-debugging-port`, через CDP
кликает чип Images, листает visible/weaker до конца, раскрывает hidden и печатает JSON. Это проверяет реальный
код окна, а не копию.

Ожидаемый результат на текущем `calibration-hits.json` (регрессионный снимок, не продуктовые константы):

```json
{"first":{"ranked":120,"total":3014,"hiddenTotal":758,"chip":"894","cutoffs":{"visible":0.648,"weak":0.6}},
 "loaded":{"ranked":894,"weaker":2120,"cells":3014,"falling":true},
 "hidden":{"hidden":758,"falling":true,"aboveWeak":true},"errors":[]}
```

`TIER_LOW=1` должен дать сдвинутые пороги (`0.4779/0.4299`), `ranked=1330`, `hidden=81`, `errors:[]`. Если числа
разошлись — сначала проверьте, не менялся ли `calibration-hits.json`, потом код.

Ограничения стенда: mock-helper, нет реального paint/декодирования миниатюр, нет WebView2. Живую проверку делайте
через dev-запуск (§5) или установленный установщик; для пагинации в живом окне использовались запросы `korean` и
`chinese` на `.build/asian-index`.

Старые стенды пагинации: `.build/make-pagination-harness.cjs` + `.build/run-pagination-cdp.cjs`
(порт 9876, переменная `PAGINATION_QUERY`), живая проверка — `.build/live-pagination-check.cjs` и
`.build/check-live-pagination.cjs`. Они локальные и не коммитятся; при потере ориентируйтесь на этот раздел и
`docs/search-results-pagination-plan.md` §11.

## 8. Как устроен поиск (минимум, который надо помнить)

Поток: выбранные папки → crawl/diff → экстракторы (изображения, текст, аудио) → модель в helper-процессе →
один SQLite-индекс (`index.sqlite`; code-поиск — отдельный `code.sqlite`) → запрос эмбеддится и смешивается с
FTS5 → страница результатов. Каждый сегмент несёт `loc`/`locEnd`, поэтому хит открывается на странице PDF, минуте
аудио или строке кода.

Главное окно (`shell/ui/window.js`) грузит **120** результатов, затем по **300**; догрузка инкрементальная, без
полной перерисовки. Панель остаётся на 40, macOS — на 120.

Протокол `search-service` (одна JSON-строка на запрос/ответ):

| Поле | Смысл |
|---|---|
| `offset`, `limit` | страница в полном рейтинге |
| `total` | файлов во всём рейтинге после смысловых гейтов и `kinds` (до проверки существования файлов) |
| `strong_total` | из них сильных: score ≥ 0.5 от глобального лучшего (в слоях Images — `visible`) |
| `strong` | сильных **в этой странице** (для старого UI) |
| `consumed` | сколько кандидатов покрыла страница до фильтрации |
| `next_offset` | `offset + consumed`; клиент двигает offset по нему, а не по `hits.length` |
| `generation` | растёт при `reload()`; страницы другой генерации отбрасываются |
| `kind_counts` / `kind_totals` | сильные / все результаты по каждому виду во всём рейтинге (чипы окна) |
| `tier_counts` | `{visible, weak, hidden}` по всему рейтингу изображений (только для слоёв Images) |

Правила, которые легко нарушить:

- `next_offset` считается по `consumed`: helper отфильтровывает несуществующие пути и исключённые типы **после**
  нарезки страницы. Если двигать offset по числу хитов, появятся пропуски.
- `strong` страницы считается по **глобальному** `topScore`, иначе вторая страница объявит сильными свои слабые
  результаты.
- Смена `generation` или `meaning` между страницами — это другой рейтинг: окно перезапускает поиск, а не смешивает
  страницы.
- `total` — верхняя оценка: исчезнувшие файлы вычитаются по мере догрузки/сверки индекса. `kind_totals` тоже
  считаются по индексу, не по `stat`.

### Слои Images (текущая fork-логика)

Фильтр Images показывает изображения не одной кучей, а тремя слоями по **cosine лучшего сегмента** (абсолютная
величина, не зависящий от запроса z-score):

- `visible` — cosine ≥ 0.648: показываются сразу;
- `weak` — 0.600 ≤ cosine < 0.648: свёрнуты как weaker matches;
- `hidden` — cosine < 0.600: отдельная свёрнутая секция, грузится своим запросом (`by_cosine`, `cosine_ceiling`).

Пороговые значения получены на живом индексе (3 772 изображения, запросы Asian/Anime): 0.648 — начало «не совсем
того», 0.600 — конец единичных нерелевантных. Если у запроса вся шкала ниже (visible < 30 изображений), окно
опускает оба порога под собственный топ: `visible = top - 0.05`, `weak = top - 0.05 - 0.048`. Это защита от
пустого первого экрана; менять константы — только с новым замером.

Запрос слоёв: `by_cosine: true`, `cosine_floor: weak`, `tier_cutoffs: [visible, weak]`. Скрытый хвост:
`cosine_ceiling: weak`. Файлы, попавшие по словам и не имеющие вектора (`keyword_rank != null`), показываются
сразу, независимо от cosine. Движок при `by_cosine` сортирует по cosine (совпадения по словам — сверху), а
`tier_counts` считает по всему `best`, включая хвост ниже floor.

Чипы видов берут числа из `kind_totals`; для Images чип показывает `visible`, то есть «сколько видно сразу», а не
весь рейтинг.

### Code-поиск

Opt-in, отдельный индекс, префикс `code:` (CLI: `--code`). При пагинации ранг для выбора «где определение» должен
оставаться глобальным (`start + hits.count`), иначе на второй странице перестанет работать правило «искать
определение только в первых 10». Материализация code-хита дороже, живой замер больших страниц ещё не сделан.

## 9. Состояние ветки и что дальше

Ветка `file-operations`, база — `main` (upstream/main `d6f1f48`). Перед коммитом 2026-10-27 origin был на
`ceeb001 Load search results a page at a time`; рабочее дерево содержало незакоммиченные слои Images, counts и
починку кэша превью — они вошли в коммит вместе с этим руководством.

Проверено на 2026-10-27:

- `python .build/swift_test.py --filter SearchPagination` — 6 тестов, все прошли (включая
  `imageLayersAreCutOnCosineAndPageInOrder` и `kindCountsCoverTheWholeRankingNotJustThePage`);
- `node --check` по всем `shell/ui/*.js` — чисто;
- headless-стенд слоёв: обычные и `TIER_LOW=1` пороги, без ошибок (числа выше);
- `cargo test` в `shell/src-tauri` — 55 тестов, все прошли после удаления updater'а.
- GitHub Actions в форке выключены (`gh workflow disable ci.yml packages.yml homebrew.yml website.yml`), default
  branch — `file-operations`; сборки только вручную (`scripts/package-portable.sh`).

Не проверено / открытые задачи:

- живой WebView2 после сборки слоёв: paint, декодирование миниатюр, поведение скрытого хвоста на больших индексах;
- code-пагинация на большом репозитории;
- индекс ≥ 50 тыс. файлов: полное сканирование и стоимость страницы (при >100 мс рассмотреть кэш рейтинга в
  helper'е);
- macOS и быстрая панель по-прежнему на 120/40; перенос пагинации на macOS — отдельный PR;
- таблица дороже сетки; виртуализация для десятков тысяч строк — отдельный этап.

## 10. Грабли, на которые уже наступали

- **`Package.resolved`** больше нет: после удаления macOS-приложения и Sparkle внешних зависимостей в пакете не
  осталось, SwiftPM сам удаляет файл. Возвращать его из истории не нужно.
- **Превью helper'а**: при старте helper удаляет `<index>/previews`, поэтому закэшированные пути в UI мертвы.
  `results.js` слушает событие `helper` (`search`/`ready`), очищает `renders` и повторяет запрос один раз со
  свежим `<img>`; после второй неудачи показывает иконку типа. Не убирайте ретрай.
- **Fingerprint индекса**: векторы помечены моделью, ревизией, runtime и dtype. Смена модели/настроек требует
  `--reindex`; смешивание источников — жёсткая ошибка.
- **`Vendor/`**: SHA-256 пины в `scripts/vendor-checksums.json`; версию и хэши менять одним коммитом, хэш не
  брать из самого скачанного файла.
- **llama.cpp**: обновление должно пройти `Tests/LlamaRuntimeTests/ReferenceVectorTests`; перегенерация эталона
  инвалидирует все индексы.
- **Приватность**: никакой телеметрии и сетевых вызовов, кроме загрузки модели и открытия страницы релизов по кнопке.
- **Windows-тесты**: symlink-тест 1314 — среда, не код.
- **Пути с пробелами**: кавычки в скриптах обязательны; `MSYS_NO_PATHCONV=1` для `cmd.exe /c`.
- **Имя сборки**: файлы называются по `productName`/`version` из `tauri.conf.json` (`DigUp Plus_0.1.0_…`).


## 11. Карта документации

| Файл | О чём |
|---|---|
| `README.md` | продукт, установка, «Build from source» |
| `CONTRIBUTING.md` | правила upstream, PR, отчёты о багах |
| `docs/search-results-pagination-plan.md` | пагинация: замеры, протокол, слои Images, как воспроизвести |
| `docs/file-operations-plan.md` | Move/Copy/Delete в окне Windows (по-русски, источник истины) |
| `docs/image-embedding-performance.md` | замеры скорости эмбеддинга, пул процессов |
| `docs/model-profiles-plan.md` | хранение модели, папка, удаление, профили |
| `AGENTS.md`, `knowledge.md` | локальные заметки агентов (не коммитятся) |
