# DigUp: файловые операции из результатов поиска

**Статус:** Phase 0 (сборка, baseline, smoke-test, shell, установщик), Phase 1 (индексные операции), Phase 2 (Rust: файловые операции), Phase 3 (UI), Phase 4 (path-aware sync, watcher-фильтр, отчёт) и edge cases Phase 5 (длинные пути, cloud placeholders, read-only, занятые файлы) закрыты. Подсказки и документация тоже; остались доступность и ручной E2E.  
**Обновлено:** 2026-10-24.  
**Платформа первого этапа:** Windows 10/11.  
**Область:** Move, Copy, Delete для медиа-файлов из результатов поиска в большом окне DigUp.

> **Если контекст потерян:** этот файл — единственный источник истины по фиче. Ключевое: Copy не пишет в индекс; Move/Delete правят индекс без переэмбеддинга; Select All = весь выданный список, а не viewport; Space остаётся preview; конфликты имён решаются авто-переименованием; Swift 6.4.0 уже установлен локально и не является runtime-зависимостью пользователя. Phase 0 закрыт: CUDA-сборка, baseline (76 файлов за 7 s на CUDA, 178 s на CPU), Unicode, аудио/видео, файл 3 GiB, запуск helper из shell и установщик (2.3, 7). Phase 1 закрыт: `file_op` в `search-service` (`IndexStore.file(at:)`, `move` с inode/device/size/mtime, `delete` в транзакции, guard `stillIndexed` в `Indexer`). Phase 2 закрыт: `file_ops.rs` (preflight, авто-переименование, `MoveFileExW`/`CopyFileExW`/Корзина, прогресс, отмена, отчёт) и Tauri-команды `file_operation`/`cancel_file_operation`/`pick_destination_folder`. Phase 4 закрыт: watcher отдаёт пути и отсекает типы, которые crawl не читает; helper принимает `sync [JSON]`/`sync code [JSON]` и пересобирает только затронутые папки; эхо-заметки на Windows точные (без родительских папок); отчёт живёт до следующей операции.

---

## 1. Цель

Пользователь нашёл набор файлов (например, все изображения с жёлтым фоном), видит их в результатах и может:

- выделить все или часть текущей выдачи;
- переместить выбранные файлы в папку по выбору;
- скопировать выбранные файлы в папку по выбору;
- удалить выбранные файлы в Корзину;
- получить понятный отчёт: что выполнено, что переименовано, что пропущено и почему.

Главное требование: **операции не должны замедлять индексацию**. Move и Delete обновляют уже существующие записи индекса без повторного чтения файлов и без эмбеддинга. Copy не пишет в индекс сам; если папка назначения индексируется, копии подхватываются обычной индексацией.

---

## 2. Принятые решения

| Вопрос | Решение |
|---|---|
| Какие файлы | Только медиа: `screenshot`, `image`, `video`, `audio`. Документы и код — вне первого среза. |
| Где | Большое окно результатов (`Ctrl+O`), не быстрая панель. |
| Платформа | Windows 10/11 первая. macOS/Linux — позже. |
| Удаление | В Корзину по умолчанию. Permanent delete в первом срезе нет. |
| Конфликт имён | Автоматически переименовать: `photo.jpg` → `photo (2).jpg`, затем `(3)`. Никогда не перезаписывать. |
| Copy и база | `Copy To` не пишет в индекс. Внутри области поиска (с учётом skipped-подпапок) копии индексируются обычным watcher/indexer; снаружи — не индексируются. Отдельного механизма/триггера нет. |
| Move и база | Обязательно править индекс: внутри области поиска — обновить путь; наружу — удалить запись. Без повторного эмбеддинга. |
| Delete и база | Удалить запись, её FTS-строки и сегменты. |
| Что считается «видимым» | Весь текущий выданный список результатов после запроса и фильтра, а не то, что поместилось в окно. Прокрутка не влияет. |
| Select All | Выбирает все медиа в текущем списке, включая файлы за пределами экрана. Не «все совпадения в индексе»: текущая выдача ограничена лимитом (сейчас 120). |
| Свёрнутые weaker | Не входят в Select All, пока не развёрнуты. После разворачивания входят. |
| Смена запроса/фильтра | Очищает выделение. Скрытые выделенные снимаются. |
| Шорткаты | Существующие не меняем. Space остаётся preview. Для выделения добавляем Insert и кнопки панели; Media-чип — `Ctrl+M`, `Ctrl+1…7` не двигаются. |
| Панель | Полноценная панель с `Select All`, `Deselect All`, `Copy To…`, `Move To…`, `Delete`, видимая при наличии медиа-результатов. |
| Выделение | Паттерны Total Commander: клик, Ctrl+клик, Shift+клик, Shift+стрелки, Insert, Ctrl+A, правая кнопка по выделению. |
| Swift | На Windows движок и CLI уже Swift (`digup.exe`). Toolchain 6.4.0 установлен (Phase 0) и собран; для пользователя это не runtime-зависимость: установщик кладёт Swift runtime DLL в `helper/`. |

### 2.1. Связанные issues и PRs

| Issue / PR | Что важно | Как учитываем |
|---|---|---|
| **#140** «Allow multi-selecting and deleting results» | Автор проекта сознательно оставляет удаление и перемещение Finder'у: «a search tool that deletes is one wrong keystroke from a bad day». Вместо этого он планирует «Hide from Search» и копирование путей. | Это **осознанное расхождение с upstream**. Безопасность компенсируем: подтверждение, Корзина, никогда не перезаписываем, отчёт по каждому файлу. Из #140 берём полезное: **Copy Paths** для выделения. «Hide from Search» в этот срез не входит. |
| **#110 / PR #114** «Sync only affected folders» | Автор хочет `sync <JSON paths>`: plain `sync` — полный, с путями — частичный. PR #114 всё ещё открыт (обновлён 2026-10-21), в main не влит. | **Сделали сами для Windows**, не дожидаясь upstream: тот же протокол (`sync [JSON]`, `sync code [JSON]`) и те же правила (`chosenRoots`, `crawlPath`, scopes). Если PR вольют — сверить реализацию и убрать расхождения. Copy по-прежнему не подавляется: копия должна индексироваться. |
| **#101** «Only sync on folder changes that can change the index» | Слито в main как `c6aed1f`, но `Rules.changeMayMatter` есть только в Apple-платформе. | **Портировано** в `watcher.rs`: документы/PDF/медиа и папки важны, `.log`/`.tmp`/базы и пакеты — нет; исключённые типы из Settings тоже. Патчи в `~/Library` и скрытые/developer-папки отсекаются раньше. |
| **#115** «Refresh active search results when the query or index changes» | Открытая выдача может оставаться старой после изменения индекса; автор не хочет, чтобы результаты двигались под курсором. | Наша фича сама убирает перемещённые/удалённые строки. Полный re-run активного запроса — отдельно, не смешивать. |
| **#144** «Explicitly selected hidden subdirectories are ignored» | `FolderSelection.isSearched` может считать явно выбранную скрытую подпапку невыбранной. | Это влияет на Move: файл, перемещённый в такую папку, можно ошибочно удалить из индекса. Перед Phase 1 проверить/починить #144 или покрыть тестом. |
| **#130** «Windows: indexing crashes on non-ASCII paths» | Открытый краш `0xC000001D` на путях с `ö`, `é`, `日` и т.п.; нужны детали от репортёра. | На текущей сборке не воспроизводится: папка `Юникод-é` с `é-файл.txt` и `日.png` проиндексирована и находится поиском (см. 2.3). Файловые операции всё равно — только wide Win32 API (`MoveFileExW`, `CopyFileExW`, `SHFileOperationW`), не ANSI; E2E с Unicode остаётся. |
| **#97 / #87** | #87 (файлы 2–4 GiB ломают crawl) исправлен через PR #100; #97 («0 files indexed» на Windows) остаётся открытым и может быть связан с GPU/логами. | Phase 0 smoke-test пройден: 80 файлов, ни одного failed, файл 3 GiB не ломает обход. Логи — `worker.log` и `costs.json` в папке индекса (`index.log`/`speed.log` в этой сборке нет). #97 на текущей сборке не воспроизводится. |
| **#61** | Cloud placeholders (OneDrive/iCloud/Google Drive) пропускаются; `dataless` — признак. | Preflight: если файл стал dataless после поиска — пропустить с понятной причиной. Copy внутрь cloud-папки может гидратировать файл; это обычное поведение, не наша индексация. |
| **#135 / PR #142** | IME: поиск не должен срабатывать на промежуточном составе; PR открыт и меняет обработку composition. | Все новые клавиши (`Insert`, `Ctrl+A`, `F5/F6/F8`, `Delete`) проверяют `event.isComposing` и не вмешиваются в IME. |
| **PR #145** | Открытый performance-PR меняет `Searcher.reload` на in-place и очередь preview. | `file_op` вызывает публичный `Searcher.reload()`, не полагается на старую реализацию. При мерже — rebase и проверка, что move/delete не возвращают старые векторы. |
| **#79** | Открытый PR исключает secret/password-файлы из индексации; медиа не затрагивает. | Не добавлять secret-документы в выборку; медиа-операции не меняются. |
| **#45** | Движок уже использует транзакции для парных записей. | Новые `file_op`-изменения — через `IndexStore` и его транзакции; не писать SQL напрямую из Rust. |

### 2.2. Состояние кода и окружения

Чтобы не изучать заново:

**Окружение этой машины**

- Windows 10, Git Bash; пути в командах — через `/`, не `\`.
- **Git Bash этого стенда (`d:/Works/Win-Bash`) нельзя использовать для запуска `digup`:** он портит PATH дочерним процессам (`C:\…` теряет `C:`), из-за чего `ggml-cuda.dll` молча не грузится. Симптом — `backends: CPU` в `worker.log` и индекс в ~25 раз медленнее, без ошибки. Он же не умеет создавать non-ASCII имена (вместо `é`/`日` получается mojibake). Запускать только из `.bat` через `cmd.exe`, где `PATH` и `DIGUP_BACKENDS` выставлены явно; тест-файлы с Unicode создавать из Python.
- Swift **6.4.0 установлен** в `C:\Users\sucot\AppData\Local\Programs\Swift` (toolchain `6.4.0+Asserts`); `swiftc` hello-world собран и запущен. PATH и `SDKROOT` прописаны в `HKCU\Environment`; в новых терминалах подхватываются. Сборка — в MSVC dev-shell: прямой `swift build` из Git Bash без `vcvars64` падает на `could not find CLI tool link`.
- GPU: **RTX 4070 Ti Super 16 GB**, `nvcc` 13.1 есть в PATH. Форк — **CUDA-only**: `scripts/build-llama.sh` собрал `llama.cpp b11461` с `FLAVOR=cuda` (см. `Vendor/llama/BUILD`), CPU-бэкенды остались только аварийным fallback (`GGML_CPU_ALL_VARIANTS=ON`). CUDA-сборка проверена: `digup speed --model` показал `backends: CUDA, CPU`, `device: gpu`, agreement `0.999755`, и каждый `index` пишет то же в `worker.log`. Vulkan не используем и не ставим; upstream CI (`packages.yml`) собирает Vulkan, но для форка это не target.
- **CUDA runtime:** `ggml-cuda.dll` зависит от `cublas64_13.dll` (а тот — от `cudart64_13.dll`). Они лежат не в `Vendor/llama/lib`, а в `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64`. Локально работают два варианта: `DIGUP_BACKENDS=Vendor/llama/lib` и CUDA `bin/x64` в PATH, или (как в установщике) все DLL рядом с `digup.exe` — тогда хватает одного `PATH` к CUDA. Без CUDA DLL backend молча падает в CPU. Для установщика эти DLL нужно явно положить в `helper/` — `package-windows.sh` их сейчас не копирует.
- **ffmpeg:** собран в MSYS2 (`C:\msys64`, gcc 16.1 + meson/ninja/nasm/pkgconf): `Vendor/ffmpeg/BUILD` = `ffmpeg 9.0.2 dav1d 1.5.2 …`, статический (15,3 и 15,5 MB), HEVC и AV1 (dav1d) на месте. Собирать только из пути без пробелов: `dav1d.pc` экранирует пробел в префиксе, и configure не находит `dav1d/dav1d.h`; рабочий вариант — junction `D:\DigUpBuild`. Системный ffmpeg 7.1.1 в `D:\Works\FFmpeg` больше не нужен. `cmp` из diffutils и `make` тоже пришлось поставить в MSYS2.
- **SwiftPM 6.4:** `swift build -c release --product digup` прошёл, но exe лежит в `.build/out/Products/Release-windows-x86_64/digup.exe`; Swift Build не смог создать симлинк `.build/release` (I/O error 512). `package-windows.sh` теперь берёт путь из `swift build -c release --show-bin-path` (печатает именно `.build/out/Products/Release-windows-x86_64`), а не из `.build/release`; проверено сборкой установщика.
- **Установщик:** `package-windows.sh` запускается из Git for Windows bash внутри dev-shell. `rustc` иначе берёт `link.exe` из `/usr/bin` (coreutils `link`) вместо MSVC; в PATH должен идти первым `VCToolsInstallDir/bin/Hostx64/x64`. С этим установщик собирается.
- **`Package.resolved`:** каждый `swift build` на Windows удаляет этот tracked-файл (наблюдалось дважды; вероятно, потому что на Windows у пакета нет зависимостей — Sparkle только для macOS). После сборки — `git checkout -- Package.resolved`, иначе попадёт в коммит как удаление.
- Rust и Node есть; `cargo test` и node-тесты доступны.
- **`swift test` на Windows:** без нужных DLL в `PATH` тест-раннер падает `abnormal(309)` (DLL not found) уже на
  `--dump-tests-json`, без имён тестов. Нужны `XCTest.dll`/`Testing.dll` из
  `…/Swift/Platforms/6.4.0/Windows.platform/Developer/Library/{XCTest,Testing}-6.4.0/usr/bin64`, `Foundation.dll` и
  остальное из `…/Swift/Runtimes/6.4.0/usr/bin`, `Vendor/pdfium/bin` и `Vendor/llama/lib`; debug-сборке — ещё
  `…/VC/Redist/MSVC/<ver>/debug_nonredist/x64/Microsoft.VC143.DebugCRT`. Ещё `Vendor/ffmpeg/bin` — иначе ImageTests
  не поворачивают TIFF/WebP/HEIC. С ними 124 теста запускаются.
- `gh` настроен; upstream — `ARahim3/DigUp`.
- Windows-сборка в CI (`packages.yml`) использует **Swift 6.4.0**, MSVC dev-cmd, Vulkan SDK и MSYS2/MinGW для ffmpeg; `Package.swift` объявляет tools 6.2. Для форка берём Swift 6.4.0 и MSVC, а Vulkan из upstream-пайплайна не тащим. Локально стоит 6.4.0, как CI.
- `ci.yml` гоняет Swift на macOS/Linux и `cargo test` shell на Linux; **Windows Swift-тесты автоматически не гоняются** — Windows E2E ручной.
- Модель — EmbeddingGemma 2, ~865 MB; первый запуск без неё только индексирует имена.

**Что уже проверено в коде**

- `Package.swift` собирает движок и `digup` на Windows; macOS-only — только `DigUpApp`. Shell запускает `helper/digup.exe`.
- `IndexStore.move(id, to:)` меняет `path`, FTS и (для `file_op`) `inode`/`device`/`size`/`mtime`; векторы не трогает.
- `IndexStore.delete(id)` удаляет FTS, files и segments (`ON DELETE CASCADE`) в транзакции; вложенный savepoint работает внутри `prune`/`sync`.
- `IndexStore.file(at:)` есть; `file_op move/delete` меняет БД под `lock` и перезагружает `Searcher`, если векторы загружены.
- `Indexer.sync(roots, changedPaths:)` — полный crawl без путей и scoped crawl с ними; `Crawler.crawl(…, chosenRoots:)` сохраняет правила выбранных папок.
- `SearchService` принимает `file_op`; `ask` в `main.rs` пускает его в whitelist.
- `shell/src-tauri/src/watcher.rs`: callback получает пути, отсекает неиндексируемые типы (`change_may_matter`) и отдаёт их `index_changed`/`code_changed`; пустой всплеск — полный sync.
- `window.js`: одно `state.selection`, `LIMIT = 120`, фильтры без Media, `actions()` только Open/Show; правый клик уже имеет Copy/Copy Path.
- `Files+Apple`: #107 в main (crawl больше не спрашивает iCloud-статус у каждой записи); #101 `changeMayMatter` есть только там. `DirectoryWalk` на Windows уже нативный после #100, Foundation за метаданными не ходит.
- `Cargo.toml`: `windows-sys` с `Win32_UI_Shell` уже есть; `Win32_Storage_FileSystem` для `MoveFileExW`/`CopyFileExW` нужно добавить фичей, не новым crate.
- `FolderSelection.isSearched` — точка истины «внутри области поиска» для выдачи; для move destination нужен `Rules.isWalked` (hidden/.noindex/developer/skipped), иначе `isSearched` и crawl расходятся; см. #144.

**Полезные команды**

```bash
# Rust/UI
cd shell/src-tauri && cargo test --locked -j 3
node --check shell/ui/window.js

# Swift — из MSVC dev-shell (cmd.exe с vcvars64.bat), не из этого Git Bash:
#   call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
#   set PATH=C:\Users\sucot\AppData\Local\Programs\Swift\Toolchains\6.4.0+Asserts\usr\bin;%PATH%
#   set SDKROOT=C:\Users\sucot\AppData\Local\Programs\Swift\Platforms\6.4.0\Windows.platform\Developer\SDKs\Windows.sdk
#   swift --version
# Без SDKROOT: "unable to load standard library for target 'x86_64-unknown-windows-msvc'".
# Прямой export PATH="C:/…" в этом bash ломает первую запись PATH, и swift не находит link.
# После swift build: git checkout -- Package.resolved (SwiftPM на Windows его удаляет).
scripts/fetch-vendor.sh
scripts/build-llama.sh            # в MSVC dev-shell (vcvars64), иначе nvcc/cl не найдутся
swift build -c release --product digup   # тоже в dev-shell: без MSVC — "could not find CLI tool link"

# Запуск собранного digup: только через cmd.exe/.bat, не из этого Git Bash.
# .bat: set PATH=<CUDA>\v13.1\bin\x64;%PATH%  и  set DIGUP_BACKENDS=%CD%\Vendor\llama\lib
# Перед замерами — проверить первую строку .build\<index>\worker.log: "backends: CUDA, CPU".

# Полная Windows-сборка, как в CI
scripts/package-windows.sh
```

---

### 2.3. Baseline и smoke-test (2026-10-23)

Машина: Windows 10 19045, RTX 4070 Ti SUPER 16 GB, driver 610.62, CUDA 13.1, Swift 6.4.0, llama.cpp `b11461` (CUDA).
Набор `.build/testbed` (под gitignore, собран Python-скриптом, не коммитится):

- `baseline/` — 41 PNG 1024×768, 10 PDF по 1 странице, 25 txt: 76 файлов, 100 векторов;
- `big/three-gig.bin` — 3 GiB (тот случай, где Foundation мог упасть на 2–4 GiB);
- `Юникод-é/` — `é-файл.txt` (с ASCII-маркером `zorblat`) и `日.png`;
- `media/` — `tone-440.wav` 3 s и `testcard.mp4` 3 s.

| Метрика | CUDA | CPU |
|---|---|---|
| первый проход, 76 файлов | **7 s** (включая 3,4 s загрузки модели) | **178 s** |
| image (41) | 0,14 s / файл, max 0,52 s | 3,47 s |
| doc (25) | 0,028 s | 1,29 s |
| pdf-страница (10) | 0,033 s | 0,33 s |
| неизменный sync (76) | **0,15 s**, `nothing pending`, модель не грузится | — |
| размер индекса | **381 KB** (100 × 768 float + FTS) | тот же |

Smoke (`.build/testbed` целиком): 80 файлов и 104 вектора за 7 s, индекс 397 KB, ни одного failed:
3 GiB файл обходится без падения (`skipped: 1 other type`); `é-файл.txt` находится поиском `zorblat` по пути
`…\testbed\Юникод-é`; `日.png`, `tone-440.wav` и `testcard.mp4` проиндексированы (image 42, audio 1, video 1).
Аудио и видео тогда прочитал системный ffmpeg 7.1.1; теперь в `Vendor/ffmpeg` лежит свой 9.0.2 (см. 2.2).

Логи: прогресс в stdout, `worker.log` и `costs.json` в папке индекса; `index.log`/`speed.log` в этой сборке нет.

Как воспроизвести: `.bat` с `set PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\x64;…;%PATH%`,
`set DIGUP_BACKENDS=%CD%\Vendor\llama\lib`, затем `digup.exe index .build\testbed\baseline --index …`; перед замером
убедиться, что `worker.log` начинается с `backends: CUDA, CPU`.

Что это значит для фичи:

- стоимость индексатора ≈ 0,05 s на файл (3,6 s работы / 76, без загрузки модели); Move/Delete 10–120 файлов
  должны укладываться в доли секунды и не добавлять ни одного эмбеддинга — сравнивать с 0,15 s неизменного sync,
  а не с полным проходом;
- полный повторный sync после ошибки `file_op` стоит ~7 s на 76 файлов — допустимый fallback;
- 3 GiB файл в индексируемой папке не ломает crawl, значит E2E с большими медиа можно не бояться.

---

## 3. Вне области первого среза

- Документы (`pdf`, `doc`) и результаты code-поиска.
- Быстрая панель поиска (маленькое окно).
- Permanent delete.
- Undo для Move/Copy. Undo для Delete — Корзина.
- macOS и Linux.
- Копирование/перемещение папок: результаты — файлы.
- Автоматическое создание папки назначения.
- Сетевые/облачные сценарии сверх обычного поведения Windows.

---

## 4. Архитектура

### 4.1. Разделение ответственности

- **Rust / Tauri shell** — файловые операции:
  - проверка путей и коллизий;
  - выбор папки назначения;
  - `MoveFileExW`, `CopyFileExW`, Корзина;
  - прогресс и отмена;
  - отчёт по каждому файлу.
- **Swift `search-service`** — индекс:
  - поиск записи по пути;
  - решение «внутри индексируемых папок или снаружи»;
  - `IndexStore.move` / `IndexStore.delete`;
  - `Searcher.reload()` после изменения;
  - ответ по каждому файлу.

Почему не писать SQLite из Rust: это дублировало бы схему, FTS и правила `IndexStore`, риск рассинхронизации. `IndexStore` уже содержит нужные операции.

### 4.2. Поток Move

1. UI собирает выбранные пути и вызывает Tauri-команду.
2. Rust проверяет:
   - файл существует и является обычным файлом;
   - это не symlink/junction и не dataless cloud placeholder (#61);
   - размер и mtime совпадают с результатом поиска (файл не изменился после поиска);
   - папка назначения существует и доступна;
   - имя свободно; если нет — подбирает `name (2).ext`.
3. Rust перемещает файл без перезаписи.
4. Rust вызывает `search-service` с `{op:"file_op", kind:"move", items:[{from,to,inode,device,size,mtime}]}`; `inode`/`device` — строки, значения уже у нового пути.
5. Swift:
   - находит запись по `from`;
   - если `to` достижим для crawler (`isCrawled`, он же `Rules.isWalked`) — `IndexStore.move(id, to:, inode:, device:, size:, mtime:)` (обновить путь, идентичность и метаданные);
   - если снаружи — `IndexStore.delete(id)`;
   - делает `searcher.reload()` только если векторы были загружены;
   - возвращает результат по каждому файлу.
6. UI убирает перемещённые строки из текущей выдачи и пересчитывает счётчики.
7. Watcher-эхо от собственного перемещения подавляется, чтобы не запускать полный sync.

Если шаг 5 не удался: watcher **не** подавляется, запускается repair-sync; UI всё равно убирает строки по старым путям.

### 4.3. Поток Copy

1. UI собирает выбранные пути.
2. Rust проверяет источник и destination, подбирает свободное имя.
3. Rust копирует файл с `COPY_FILE_FAIL_IF_EXISTS`, прогрессом и отменой.
4. **В индекс не пишет.**
5. Если папка назначения внутри области поиска (с учётом skipped) — обычный watcher/indexer добавит копии. Если снаружи — они не появятся в поиске.
6. UI оставляет текущие строки, очищает выделение и показывает отчёт.

### 4.4. Поток Delete

1. UI собирает выбранные пути, показывает подтверждение.
2. Rust по одному файлу отправляет путь в Корзину.
3. Для каждого успешного файла Rust вызывает `file_op` с `kind:"delete"`.
4. Swift удаляет запись из `IndexStore` (FTS и сегменты уходят).
5. UI убирает удалённые строки; неудачные остаются.
6. Watcher-эхо подавляется только для успешно удалённых файлов.

### 4.5. Watcher

Сейчас `FolderWatcher` при любом изменении вызывает `run_index`, то есть полный обход. Для Move/Delete это лишняя работа: база уже обновлена. План:

- передавать в callback пути события, а не только сигнал;
- хранить короткоживущий набор «наших» путей (source + destination + родительские папки) с дедлайном;
- если весь всплеск состоит только из этих путей — не запускать sync;
- если в всплеске есть чужой путь — запускать обычный sync;
- для Copy ничего не подавлять: копии должны индексироваться, если папка внутри области поиска;
- при ошибке обновления индекса ничего не подавлять.

Это не новый триггер индексации, а подавление эха уже выполненной операции.

### 4.6. Конкретные решения до реализации

Ниже — наброски, по которым делались Phase 1–4. Swift-часть (Phase 1), Rust-часть (Phase 2), UI (Phase 3) и
watcher (Phase 4) реализованы и собираются; чем они отличаются от наброска — в 4.6.3, 4.6.4 и в Phase 1–4.
Актуальный статус — в разделах Phase 3–4.

#### 4.6.1. Swift: `IndexStore`

Добавить точный поиск записи по пути и по id (сейчас есть только `files(under:)`):

```swift
public func file(at path: String) throws -> FileRecord? {
    var found: FileRecord?
    try db.query("SELECT \(Self.fileColumns) FROM files WHERE path = ?", [.text(path)]) {
        found = Self.record($0)
    }
    return found
}

public func file(_ id: Int64) throws -> FileRecord? {
    var found: FileRecord?
    try db.query("SELECT \(Self.fileColumns) FROM files WHERE id = ?", [.int(id)]) {
        found = Self.record($0)
    }
    return found
}
```

`move` расширить идентичностью и метаданными нового места; иначе move между томами оставляет старые `inode`/`device` и следующий sync может переэмбеддить файл:

```swift
public func move(_ id: Int64, to path: String, inode: UInt64? = nil, device: UInt64? = nil,
                 size: Int64? = nil, mtime: Double? = nil) throws {
    try db.transaction {
        try db.run("""
            UPDATE files SET path = ?, inode = coalesce(?, inode), device = coalesce(?, device),
                size = coalesce(?, size), mtime = coalesce(?, mtime) WHERE id = ?
            """, [.text(path),
                  inode.map { .int(Int64(bitPattern: $0)) } ?? .null,
                  device.map { .int(Int64(bitPattern: $0)) } ?? .null,
                  size.map { .int($0) } ?? .null,
                  mtime.map { .double($0) } ?? .null, .int(id)])
        try db.run("""
            UPDATE fts SET name = ?, body = ? WHERE rowid IN
                (SELECT id FROM segments WHERE file = ? AND kind = 'name')
            """, [.text(SearchText.normalized(Self.nameText(path))),
                  .text(SearchText.normalized(Self.folderText(path))), .int(id)])
    }
}
```

`delete` обернуть в транзакцию (сейчас два `DELETE` идут порознь; `prune`/`sync` уже вызывают его внутри транзакции, а `Database.transaction` умеет savepoint):

```swift
public func delete(_ id: Int64) throws {
    try db.transaction {
        try db.run("DELETE FROM fts WHERE rowid IN (SELECT id FROM segments WHERE file = ?)", [.int(id)])
        try db.run("DELETE FROM files WHERE id = ?", [.int(id)])
    }
}
```

Последствия:

- `file(at:)` — точное сравнение SQLite; `from` всегда берётся из `SearchHit.path`, то есть из этой же БД, поэтому совпадает.
- `move` с `inode`/`device`/`size`/`mtime` не переэмбеддит; если размер изменился (файл менялся во время move), вызывающий должен решить, не requeue ли.
- `delete` внутри `prune`/`sync` — вложенный savepoint, не отдельный BEGIN; это поддерживается `Database.transaction`.
- `file(at:)` не чинит path-спеллинги; destination нормализуется в Rust/Swift до `/` и сравнивается с областью поиска без учёта регистра на Windows.
- `file(at:)`/`file(id:)` возвращают и `state`, и `detail`; `file_op` не должен отказываться от move только потому, что строка `failed` или `partial`.
- `move` не трогает `info` (ширина, длительность, страницы): они описывают содержимое, а оно не изменилось.

#### 4.6.2. Swift: `file_op` в `SearchService`

Добавить op в `ask` whitelist и в `SearchService.handle`:

```json
{"op":"file_op","kind":"move","items":[{"from":"C:/a/x.jpg","to":"C:/b/x.jpg",
  "inode":"123","device":"45","size":1234,"mtime":1712345678.123}]}
{"op":"file_op","kind":"delete","items":[{"from":"C:/a/x.jpg"}]}
```

`inode`/`device` — **строки**, не JSON-числа: 64-битные file id на Windows не проходят через JS/JSON без потери точности. `copy` этим op не проходит: Copy в индекс не пишет.

Скелет обработчика:

```swift
case "file_op":
    guard request["code"] as? Bool != true else { throw CLIError("file_op is for media") }
    let kind = request["kind"] as? String ?? ""
    let items = request["items"] as? [[String: Any]] ?? []
    var results: [[String: Any]] = []
    var changed = false
    for item in items {
        let from = item["from"] as? String ?? ""
        guard let row = try store.file(at: from) else {
            results.append(["from": from, "ok": true, "index": "missing"])
            continue
        }
        switch kind {
        case "delete":
            try store.delete(row.id)
            changed = true
            results.append(["from": from, "ok": true, "index": "removed"])
        case "move":
            let to = item["to"] as? String ?? ""
            if isCrawled(to) {
                if let stale = try store.file(at: to), stale.id != row.id { try store.delete(stale.id) }
                try store.move(row.id, to: to, inode: u64(item["inode"]), device: u64(item["device"]),
                               size: (item["size"] as? NSNumber)?.int64Value,
                               mtime: (item["mtime"] as? NSNumber)?.doubleValue)
                changed = true
                results.append(["from": from, "to": to, "ok": true, "index": "moved"])
            } else {
                try store.delete(row.id)
                changed = true
                results.append(["from": from, "to": to, "ok": true, "index": "removed"])
            }
        default:
            throw CLIError("unknown file_op kind")
        }
    }
    if changed, searcher.vectorsLoaded { try searcher.reload() }
    return ["results": results]
```

Каждый `item` — в своём `do/catch`: ошибка одной строки (stale, busy, удалили строку параллельно) не отменяет остальные; после цикла `changed` запускает reload, а если reload упал, ответ несёт `ok:false` и shell делает repair. Общий `ok` — только если не было ни одной ошибки.

Здесь `isCrawled(_:)` — не `FolderSelection.isSearched`, а проверка «crawler действительно дойдёт до папки назначения»: для каждого root вызывается `Rules.isWalked(path, root:home:excluded:)` (он учитывает hidden, `.noindex`, developer-папки, `~/Library`, skipped и cache-root). Это важно для #144 и для move в скрытую/пропущенную подпапку: `isSearched` может сказать «внутри», а crawl туда не ходит, и следующий sync удалит строку. На Windows пути сравниваются без учёта регистра и с `/`; иначе `C:\Foo` и `C:/foo` дают разные ответы. `u64(_:)` разбирает строку; без строковой передачи 64-битного id точность теряется.

```swift
private func isCrawled(_ path: String) -> Bool {
    let home = FileManager.default.homeDirectoryForCurrentUser.path
    // The destination folder, not the file: a file named `.photo.jpg` is still a file the crawl can classify.
    let target = FolderSelection.realPath(FolderSelection.parent(path))
    return roots.contains { root in
        let realRoot = FolderSelection.realPath(root)
        #if os(Windows)
        return Rules.isWalked(target.lowercased(), root: realRoot.lowercased(), home: home.lowercased(),
                              excluded: skipped.map { $0.lowercased() })
        #else
        return Rules.isWalked(target, root: realRoot, home: home, excluded: skipped)
        #endif
    }
}
```

`searcher.reload()` нужен только если векторы уже загружены: иначе старых векторов в памяти нет, а удалять нечего. Если reload упал — вернуть ошибку, UI запускает repair.

#### 4.6.3. Гонка с индексером

Это главный последственный риск: `IndexService` читает файл в `Indexer.prepare` и пишет `store.finish`/`addSegment` в `commit`, а `file_op` может изменить/удалить ту же строку между этими шагами. Сейчас `addSegment` при `foreign_keys = ON` получит FK-ошибку, `commit` поймает её и пометит файл failed (лишний failed вместо «файл ушёл»); а ошибка БД в самом `commit`/`finish` уже даёт fatal в `IndexService.run`. Guard убирает и ложный failed, и окно для fatal.

Решение — не пауза индексатора, а проверка актуальности перед записью:

```swift
private func stillIndexed(_ file: FileRecord) throws -> Bool {
    guard let row = try store.file(file.id) else { return false }
    return row.path == file.path && row.size == file.size && abs(row.mtime - file.mtime) < 0.001
}
```

- в `commit` пропускать `outcomes`, если `stillIndexed` false (не помечать перемещённую строку failed);
- в начало `images`, `pdf`, `doc`, `audio`, `video`, `code` добавить такой же guard; файл, которого уже нет, даёт `skipped`, а не ошибку;
- **не** проверять `fileExists` на каждом `addSegment`: это SELECT на каждый сегмент и замедление индексации. Проверка — один раз на файл; если строку удалили в узком окне после неё, FK-ошибку ловить как «файл исчез», а не как fatal, и не повторять весь batch;
- В реализации Phase 1 отдельный `finish(onlyIfPath:)` не понадобился: guard стоит в начале `commit`, а `finish`
  пишет по id — у удалённой строки обновлять нечего, у перемещённой это тот же самый файл. Остаётся узкое окно
  между guard и записью; его ловит catch по `store.file(id) == nil` (строка ниже про FK-ошибку).

Тест: начать индексацию большого файла, во время неё сделать move/delete; индекс-хелпер не падает, строка либо имеет новый путь, либо удалена, а следующая индексация не ставит false-failed.

#### 4.6.4. Rust: команда, диалог, Windows API

Tauri-команда не должна блокировать UI-поток; работа — в `spawn_blocking`, прогресс — событиями в `main`:

```rust
#[tauri::command]
async fn file_operation(app: AppHandle, shell: State<'_, Arc<Shell>>, request: Value) -> Result<Value, String> {
    let shell = shell.inner().clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || file_ops::run(&app2, &shell, request))
        .await.map_err(|e| e.to_string())?
}
```

`file_ops::run` шлёт `app.emit_to(WINDOW, "file-op", json!({...}))` на фазах `start`, `progress`, `done`; `cancel_file_operation(id)` ставит `AtomicBool`. `pick_destination_folder` — по образцу `pick_folders`, но `pick_folder` (один выбор) и заголовком `Move to Folder` / `Copy to Folder`.

Rust отправляет `to` в БД как `to_string_lossy().replace('\\', "/")`, а для Win32/`PathBuf` использует исходный путь: `C:\Foo` и `C:/Foo` — один и тот же файл, но только `/`-форма совпадает с путями индекса. `size`/`mtime` — из `metadata` уже после move.

Windows-вызовы (фича `Win32_Storage_FileSystem`):

```rust
// Move: без MOVEFILE_REPLACE_EXISTING; cross-volume — COPY_ALLOWED
unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_COPY_ALLOWED) };

// Copy: create-new + progress + cancel
let mut cancel: BOOL = 0;
unsafe { CopyFileExW(from.as_ptr(), to.as_ptr(), Some(progress), ctx, &mut cancel, COPY_FILE_FAIL_IF_EXISTS) };

// Trash: один файл, двойной NUL, Корзина; системное предупреждение не подавляем
let mut from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
let mut op = SHFILEOPSTRUCTW {
    hwnd: std::ptr::null_mut(), wFunc: FO_DELETE, pFrom: from.as_ptr(), pTo: std::ptr::null(),
    fFlags: (FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI) as u16,
    fAnyOperationsAborted: 0, hNameMappings: std::ptr::null_mut(), lpszProgressTitle: std::ptr::null(),
};
let code = unsafe { SHFileOperationW(&mut op) };
```

Проверено по документации `windows-sys 0.61.2`: `MoveFileExW`/`CopyFileExW` и константы `MOVEFILE_COPY_ALLOWED = 2`, `COPY_FILE_FAIL_IF_EXISTS = 1`, `FOF_ALLOWUNDO = 64`, `FO_DELETE = 3` там есть. `SHFILEOPSTRUCTW` поля именно такие; `fFlags` — `u16`.

Последствия:

- `MoveFileExW` без `REPLACE_EXISTING` не перезапишет; при коллизии — auto-rename и повтор.
- `CopyFileExW` с `FAIL_IF_EXISTS` атомарно не даст перезаписать; при гонке — следующий номер.
- `SHFileOperationW` может показать системное предупреждение, если файл не помещается в Корзину; это правильно, не подавляем.
- `SHFileOperationW` устарел; если per-file отчёт окажется ненадёжным, fallback — `IFileOperation`.
- long paths: `MoveFileExW`/`CopyFileExW` принимают `\\?\`; `SHFileOperationW` — хуже; для Корзины длинные пути тестировать отдельно.

#### 4.6.5. UI: конкретные точки

Состояние: `state.selected = new Set()`, `state.anchor = null`; `state.selection` остаётся focus/preview, чтобы не ломать существующее поведение.

```js
const MEDIA = new Set(["screenshot", "image", "video", "audio"]);
function operable() {
  const seen = new Set();
  return rows().filter((hit) => {
    if (!MEDIA.has(hit.kind) || seen.has(hit.path)) return false;
    seen.add(hit.path);
    return true;
  });
}
```

- `cell`/`codeRow`: `checked` класс из `state.selected`, чекбокс-кнопка с `aria-checked`; `codeRow` не получает чекбокс.
- `wire`: `item.onclick = (event) => select(hit.path, event)`; `Ctrl/Cmd` — toggle, `Shift` — range по `operable()`, обычный клик — один.
- `Insert` — toggle текущего и `moveFocus(1)`; `Shift+Arrow` — range; `Ctrl+A` — все `operable()`, только если `document.activeElement !== $query`; `Ctrl+Shift+A` — снять; `Esc` — сначала снять выделение; `F5/F6/F8` и `Delete` — действия с подтверждением.
- Панель: `<section id="batch">` над `.status`; `Select All`, `Deselect All`, `Copy To…`, `Move To…`, `Delete`; `Copy Paths` — в контекстном меню/переполнении. Media-чип — `Ctrl+M`, чтобы не менять существующие `Ctrl+1…7`.
- `renderContent` сейчас сбрасывает `scrollTop = 0`; после удаления строк сохранять `scrollTop` и фокус на следующем видимом результате.
- `renderSide`: `selected.size > 1` — сводка (кол-во, размер, первые миниатюры); иначе preview; `stopPlaying()` при смене выделения.
- `renderStatus`: при выделении показывать `N selected · size`; иначе текущий счётчик.
- `renderBar`/`counts`: Media-счётчик — сумма четырёх видов; после move/delete пересчитать `state.counts` из оставшихся `ranked`, иначе чипы врут. Не сдвигать `FILTERS`: Media получает отдельный `Ctrl+M`, а `Ctrl+2…6` остаются за screenshots/images/documents/video/audio; иначе обещанное «шорткаты не меняем» ломается.
- drag: `item.onmousedown` уже `preventDefault`; чекбокс должен `stopPropagation`, иначе начнётся drag вместо выбора.

#### 4.6.6. Матрица последствий

| Решение | Что затрагивает | Последствие | Проверка |
|---|---|---|---|
| `file_op` в search-service под `lock` | поиск, БД | поиск ждёт короткую запись; `reload` только при загруженных векторах | тест: поиск во время move |
| `IndexStore.delete` в транзакции | `prune`, `sync`, file_op | вложенные savepoints; FTS+files атомарны | тест: delete внутри prune |
| `move` с inode/device/size/mtime | cross-volume, sync | следующий sync не переэмбеддит; mtime не откатывает состояние | тест: move на другой том и обратно |
| stale destination row | UNIQUE path | Rust не перезаписывает, поэтому строка на `to` — от исчезнувшего файла; её удаляем перед move | тест: stale row + move |
| guard в Indexer | индексация | move/delete во время чтения не валит helper и не ставит false-failed | тест: большой файл + move/delete |
| path `/` + case-insensitive isCrawled | move destination | `C:\Foo` и `C:/foo` не считаются разными | тест: Unicode/турецкий `ı` |
| `Rules.isWalked` вместо `isSearched` | move destination | hidden/skipped/.noindex не считаются областью поиска, если crawler туда не ходит | тест: move в `.gallery`, `node_modules` |
| move через границу классификации | `kind` | если destination меняет kind (например, `.png` в code-проект), следующий sync может requeue и переэмбеддить; это редко и корректно | тест: `.png` в code-проект |
| #144 hidden subdir | Settings, move destination | `isWalked` спасает Move, но сам Settings всё ещё должен показывать явно выбранную скрытую подпапку | тест: выбрать `.gallery` в Settings |
| watcher handled paths | move/delete | нет полного sync от эха; copy не подавляется | тест: move 10 файлов → нет `scanning` |
| UI scroll/focus | результаты | после удаления не прыгаем в начало | E2E: удалить из середины |
| counts/selection | чипы, панель | после move/delete счётчики и выделение не врут | E2E: удалить 3 из 10 |
| checkbox + drag | мышь | клик по чекбоксу не начинает drag | E2E: чекбокс на карточке |
| IME | клавиатура | новые шорткаты не мешают composition | E2E: японский IME |
| `Ctrl+A` в поле | поиск | выделяет текст, а не файлы | E2E: фокус в query |
| `SHFileOperationW` large file | Корзина | системное предупреждение вместо тихого permanent delete | тест: файл > Корзины |

#### 4.6.7. Что это меняет в плане

- Phase 1 добавляет не только `file_op`, но и **guard в Indexer** и `file(id:)`/`file(at:)`/`move` с идентичностью; это отдельный риск для core, тесты обязательны.
- Phase 2 не делает Copy через Swift; Copy — только Rust + обычная индексация.
- Phase 3 добавляет `state.selected`/`anchor`, `Media` на `Ctrl+M`, сохранение scroll и пересчёт counts.
- Phase 4 получает точную проверку: move/delete не вызывают `scanning`; copy вызывает обычный sync.
- Перед Phase 1 нужно **выбрать поведение #144**: `isCrawled` уже не даёт Move оставить строку для пути, куда crawler не ходит, но если явно выбранная скрытая папка должна индексироваться, её всё равно надо отдельно проверить в Settings/`Crawler`. Безопасный fallback для первого среза: move в такую папку удаляет строку и пишет «destination isn't indexed»; copy туда не индексируется. Это решение, а не блокер для остального плана.
- `isCrawled` не повторяет `Rules.datasetFolders`: папка, которая после перемещения выглядит датасетом (сотни машинно-названных медиа), для `Rules.isWalked` достижима, а crawl её пропустит. Это новый открытый вопрос: либо принять, что следующий sync уберёт строку, либо добавить проверку dataset в `isCrawled`; для первого среза достаточно теста и честного сообщения в отчёте.
- `isCrawled` проверяет **папку назначения**, а не имя файла: иначе файл `.photo.jpg` ошибочно считался бы скрытым. Для `Rules.isWalked` это осознанное отличие от watcher-проверки.
- Move не пересчитывает `kind`: если папка назначения меняет классификацию (например, скриншот в code-проекте), `Indexer.sync` увидит `row.kind != file.kind` и поставит файл в очередь. Это не ошибка move, но обещание «без переэмбеддинга» действует только для move в пределах той же классификации; в тесте это надо назвать явно.

---

## 5. Поведение индекса

| Операция | Назначение | Файловая система | Индекс | Повторный эмбеддинг |
|---|---|---|---|---|
| Move | внутри области поиска | перемещение | `path` у существующей записи | нет |
| Move | снаружи области поиска | перемещение | удаление записи | нет |
| Copy | внутри области поиска | копирование | ничего; обычная индексация позже | да, как у нового файла |
| Copy | снаружи области поиска | копирование | ничего | нет |
| Delete | любое | Корзина | удаление записи, FTS, сегментов | нет |

`IndexStore.move` уже документирован как «Only the path changes; nothing is re-embedded». `IndexStore.delete` удаляет FTS-строки и файл, сегменты уходят каскадом; в `file_op` оба — в транзакции.

Для move между томами одной смены `path` мало: `inode`/`device` нового места должны попасть в строку, иначе следующий sync не сопоставит файл по inode и переэмбеддит его. `size`/`mtime` нового места обновляются там же, чтобы cross-volume copy не выглядел как изменение содержимого. Для move в пределах тома значения те же.

«Внутри» здесь и ниже — это `isCrawled` (`Rules.isWalked` по каждому root): hidden, `.noindex`, developer-папки, `~/Library`, skipped и cache-root не считаются областью поиска, даже если `FolderSelection.isSearched` говорит «внутри».

---

## 6. UI

### 6.1. Фильтр Media

Добавить чип **Media** рядом с All:

- виды: `screenshot`, `image`, `video`, `audio`;
- счётчик — сумма четырёх видов;
- не заменяет All: All показывает всё, Media — только медиа;
- в Media панель действий доступна всегда;
- в All выбираются только медиа; документы не получают чекбокс;
- в Documents/Code панель действий скрыта.

Media — `Ctrl+M`, рядом с All, но существующие `Ctrl+1…7` не меняются; подпись чипа показывает `Ctrl+M`.

### 6.2. Панель действий

Над статус-баром, видна при наличии медиа-результатов:

```
12 selected · 3.4 GB
[Select All] [Deselect All]        [Copy To…] [Move To…] [Delete]
```

- Без выделения: подсказка «Select files to copy, move, or delete», кнопки действий неактивны.
- `Select All` — все медиа текущего списка, включая off-screen.
- `Deselect All` — снять всё.
- `Delete` — красная, всегда с подтверждением.
- Счётчик и размер — по всему выделению, не по видимой области.
- `Copy Paths` для выделения — в контекстном меню и в переполнении панели (issue #140: пути в терминал).

### 6.3. Выделение

Состояние:

- `selected: Set<path>` — выбранные файлы;
- `anchor: path | null` — начало диапазона;
- `focus: path | null` — курсор/preview (существующий `state.selection`).

Правила:

- одиночный клик — выбрать только этот файл и поставить курсор;
- `Ctrl+клик` — добавить/убрать, курсор на кликнутом;
- `Shift+клик` — диапазон от anchor;
- `Shift+↑/↓` — расширить выделение;
- `Insert` — выделить/снять и перейти к следующему;
- `Ctrl+A` — Select All, только когда фокус не в поле поиска; в поле Ctrl+A выделяет текст;
- `Ctrl+Shift+A` — Deselect All;
- `Esc` — сначала снять выделение, потом закрыть preview/очистить запрос;
- `Delete` — удаление выделения с подтверждением;
- `F5` / `F6` / `F8` — Copy To / Move To / Delete;
- `Space` — preview, как сейчас; существующие шорткаты не меняем.

Все новые клавиши проверяют `event.isComposing` и не срабатывают во время IME-композиции (PR #142/#135).

Выбор по `path`, а не по конкретному моменту видео: один файл — одна операция.

Визуально:

- курсор — контур;
- выбранные — заливка + галочка;
- чекбокс появляется на hover/focus;
- `aria-selected`, `aria-multiselectable`;
- панель и модалки доступны с клавиатуры.

### 6.4. Границы «видимого»

`rows()` = `ranked` + `weaker`, если weaker развёрнуты.  
`operable()` = медиа из `rows()`.

- Select All выбирает `operable()`, а не DOM-элементы во viewport.
- Список ограничен текущим лимитом выдачи (120).
- Смена запроса/фильтра очищает `selected`.
- Сворачивание weaker убирает из `selected` пути, которых больше нет в `rows()`.
- Прокрутка и изменение размера окна выделение не меняют.
- Если путь встречается несколько раз (разные моменты одного видео), он учитывается один раз.

### 6.5. Move To / Copy To

1. Нативная папка назначения.
2. Лист подтверждения:
   - операция;
   - число файлов;
   - общий размер;
   - путь назначения;
   - сколько файлов будет переименовано.
3. Конфликт:
   - `photo.jpg` → `photo (2).jpg`;
   - сравнение имён без учёта регистра (Windows);
   - расширение сохраняется;
   - при гонке — повторить со следующим номером;
   - никогда не перезаписывать.
4. Прогресс: `Copying 7 of 12 — filename`, отмена между файлами; для Copy — по чанкам.
5. Отчёт: `10 copied · 2 renamed · 1 failed (in use)` с финальными именами и причиной.

### 6.6. Delete

- Подтверждение: `Move 12 files to the Recycle Bin?` + размер.
- Основная кнопка — «Move to Recycle Bin», не «Delete forever».
- Если файл слишком большой для Корзины, Windows может показать системное предупреждение; не подавлять его молча.
- После — отчёт по каждому файлу.
- Неудачные файлы остаются в результатах и в выделении.

### 6.7. После операции

- Move/Delete: убрать затронутые строки из `ranked`/`weaker`, пересчитать счётчики чипов.
- Copy: строки не добавлять, чтобы не дублировать выдачу; очистить выделение; статус «Copied 12 files · searchable after indexing» (если папка внутри области поиска).
- Ошибка индекса: строки по старым путям всё равно убрать, показать ошибку и запустить repair-sync.
- Статус-бар: «Moved 10 · 2 renamed», «Deleted 10», «Index update failed; resyncing».

### 6.8. Правая кнопка

Если кликнули по выбранному — действия применяются ко всему выделению. Если по невыбранному — сначала выбрать его, затем меню.

Пункты:

- Open;
- Show in Explorer;
- Copy (файл в буфер);
- Copy Paths (все выбранные, по одному на строку);
- Move To…;
- Copy To…;
- Delete (Recycle Bin).

---

## 7. Поэтапный план

### Phase 0 — Toolchain и baseline

**Статус:** всё, кроме проверки установщика на чистой машине, сделано.

- **Готово:** Swift **6.4.0** для Windows установлен (как в CI `packages.yml`; `Package.swift` совместим с tools 6.2). `swiftc` hello-world собран и запущен; PATH/`SDKROOT` — в `HKCU\Environment`.
- **Готово:** MSVC dev-shell (`C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat`); `cl` в текущем PATH не появился, но `build-llama.sh` в dev-shell собрался.
- **Готово:** `scripts/build-llama.sh` собрал **CUDA** (`Vendor/llama/BUILD` = `b11461 … cuda`); CPU — только fallback. Релизный установщик форка тоже собираем с CUDA, а не с Vulkan; upstream CI с Vulkan не является паритетом.
- **Готово:** `scripts/fetch-vendor.sh` и CUDA runtime для запуска (`CUDA/v13.1/bin/x64` в PATH плюс `DIGUP_BACKENDS=Vendor/llama/lib`, или те же DLL рядом с exe). MSYS2/MinGW для маленького ffmpeg поставлены (`C:\msys64`); Vulkan SDK не нужен.
- **Готово:** `swift build -c release --product digup` (exit 0); exe — `.build/out/Products/Release-windows-x86_64/digup.exe`, `.build/release` symlink не создался (I/O 512). `digup --help`, `digup status` и `digup speed --model` работают; CUDA backend подтверждён.
- **Готово, baseline** (Windows 10 19045, RTX 4070 Ti SUPER 16 GB, driver 610.62, CUDA 13.1, Swift 6.4.0):
  тест-набор `.build/testbed/baseline` — 41 PNG 1024×768, 10 PDF по 1 странице, 25 txt (76 файлов, 100 векторов):
  - CUDA (`backends: CUDA, CPU`): **7 s** на 76 файлов, включая загрузку модели 3,4 s; на файл — image 0,14 s (max 0,52),
    doc 0,028 s, pdf 0,033 s.
  - CPU (тот же набор, без CUDA): **178 s**, image 3,47 s, doc 1,29 s, pdf 0,33 s. Разница ~25× по общему времени
    (и ~48× по самой работе, без загрузки модели); без CUDA baseline бессмыслен.
  - Индекс: **381 KB** (100 × 768 float + FTS); повторный sync без изменений: **0,15 s**, `nothing pending`, модель
    не загружается.
  - Логи: прогресс — в stdout, плюс `worker.log` и `costs.json` в папке индекса; файлов `index.log`/`speed.log` нет.
- **Готово, smoke-test:** `.build/testbed` = baseline + `big/three-gig.bin` (3 GiB) + `Юникод-é/é-файл.txt` и `Юникод-é/日.png`
  + `media/tone-440.wav` и `media/testcard.mp4`: 80 файлов и 104 вектора за 7 s, ни одного failed; 3 GiB файл
  обходится без падения (`skipped: 1 other type`); поиск `zorblat` находит `é-файл.txt` по пути `…\testbed\Юникод-é`;
  `日.png`, `tone-440.wav`, `testcard.mp4` проиндексированы (image 42, audio 1, video 1).
- **Готово, gotcha окружения:** Git Bash этого стенда (d:/Works/Win-Bash) ломает non-ASCII имена при создании файлов и
  портит PATH дочерним процессам (`C:\…` теряет `C:`). Симптом: `ggml-cuda.dll` не грузится, `worker.log` пишет
  `backends: CPU`, индекс считается в 25 раз медленнее без ошибки. Запускать `digup` из `.bat` через `cmd.exe`, где PATH
  и `DIGUP_BACKENDS` выставлены явно; тест-файлы с Unicode создавать из Python, не из этого bash.
- **Готово, ffmpeg:** MSYS2 + `scripts/build-ffmpeg.sh` → `Vendor/ffmpeg/bin/ffmpeg.exe` и `ffprobe.exe` (9.0.2,
  dav1d 1.5.2, статические, HEVC/AV1). Собирать только из пути без пробелов: junction `D:\DigUpBuild` → проект
  (иначе `dav1d.pc` с `\ ` в префиксе и configure не находит `dav1d/dav1d.h`).
- **Готово, shell:** `cargo build --release` → `digup-shell.exe`; запущен на `.build/testbed/baseline`: поднял
  `search-service` и `index-service` из `helper/`, проиндексировал 76 файлов (100 векторов), `costs.json` —
  `device: gpu` (CUDA DLL рядом с helper, без PATH). Процессы после проверки завершены.
- **Готово, установщик:** `package-windows.sh` (bin path из `--show-bin-path`, CUDA DLL в `helper/`) собрал
  `shell/src-tauri/target/release/bundle/nsis/DigUp_0.7.0-beta.3_x64-setup.exe` (419 MiB, `package_exit=0`);
  в `installer.nsi` есть `helper\cublas64_13.dll`, `cudart64_13.dll`, `cublasLt64_13.dll`, `ffmpeg.exe`.
- **Готово, лицензия CUDA:** `notices.py` берёт EULA и notices cuBLAS из той же CUDA, чьи DLL кладёт установщик
  (`CUDA_RUNTIME_DIR`, его выставляет `package-windows.sh` по той же логике, что и копирование DLL); в
  `THIRD-PARTY-NOTICES.txt` появился раздел «NVIDIA CUDA runtime v13.1» (Attachment A перечисляет `cublas*`/`cudart*`
  как распространяемые), файл — 814 KB. На Linux раздел не добавляется: там CUDA не едет.
- **Осталось до релиза:** прогнать установщик на чистой машине/VM — здесь не стали: уже стоит DigUp в
  `d:\works\DigUp`, установщик перезаписал бы его uninstall-запись.

**Готово когда:** `digup` собирается и запускается на Windows; baseline зафиксирован; Unicode и 2–4 GiB проходят.  
**Оценка:** 1–1,5 дня.

### Phase 1 — Swift: индексные операции

**Статус:** закрыт (2026-10-24). Всё ниже — в коде; тесты на временной SQLite зелёные, сквозная проверка через
настоящий `search-service` прошла.

**Сделано:**
- `IndexStore.file(at:)` и `file(_ id:)` — запись по точному пути и по id (пути выдачи берутся из этой же БД, поэтому
  сравнение совпадает байт в байт).
- `IndexStore.delete` — FTS и files в одной транзакции (savepoint внутри `prune`/`sync`).
- `IndexStore.move(_:to:inode:device:size:mtime:)` — путь, идентичность и метаданные нового места, ничего не
  эмбеддится. `sync` по-прежнему зовёт `move(row.id, to:)`; `file_op` — с новыми значениями (cross-volume).
- `Indexer.stillIndexed(_:)` — один SELECT на файл в начале `commit`. Файл, перемещённый или удалённый во время
  чтения, не получает `finish`/сегментов и в событии идёт как `skipped` («moved or deleted while it was being read»),
  а не `failed`. Отдельный `finish(onlyIfPath:)` не понадобился: запись идёт по id, а удалённая строка просто не
  обновляется; если строка исчезла между проверкой и записью, FK-ошибка `addSegment` в catch превращается в тот же
  `skipped` (по `store.file(id) == nil`), а не в false-failed.
- `SearchService`: op `file_op` (`move`/`delete`) с результатом по каждому файлу; `move` решается `isCrawled` —
  `Rules.isWalked` по каждому root (hidden/`.noindex`/skipped/developer-папки не считаются областью поиска), на
  Windows пути сравниваются без учёта регистра; stale-строка на `to` удаляется перед move; `searcher.reload()` —
  только если векторы были загружены, и упавший reload делает ответ `ok:false` (shell уйдёт в repair). `file_op`
  разрешён в whitelist `ask` (`shell/src-tauri/src/main.rs`).
- Тесты: move с новыми `inode`/`device`/`size`/`mtime` (векторы и FTS-имя на месте, строка не переочередилась);
  `stillIndexed` до и после move/delete; delete убирает FTS и сегменты.
- Сквозная проверка `.build/phase1-check.py` (не коммитится): `digup index --limit 0` на три файла, затем настоящий
  `digup search-service`. Move внутрь root → `index:"moved"`, путь/inode/device/size/mtime обновлены, сегменты прежние,
  keyword-имя новое; delete → `"removed"`; move в папку вне root → `"removed"` (строка удалена). PASS.

**Проверено (Windows, 2026-10-24):** `swift build --product digup` — exit 0. `swift test --filter IndexStoreTests` —
12 тестов, 11 прошли; один падает по окружению (`modelFilesComeFromTheHuggingFaceCacheWhenThere` — symlink, Win32
1314, нужен Developer Mode). После Phase 4 весь набор — 124 теста: единственный провал `DigUpKitTests` — тот же
symlink; `ReferenceVectorTests` не запускается без модели. `ImageTests` (HEIC/TIFF/WebP) проходят, если `Vendor/ffmpeg/bin`
в `PATH`; без ffmpeg они падали так же и до Phase 1.

**Готово когда:** move/delete обновляют индекс без переиндексации; тесты зелёные.  
**Оценка:** 1–1,5 дня.

### Phase 2 — Rust: файловые операции

**Статус:** закрыт (2026-10-24). Код собран и покрыт тестами; UI ещё нет (Phase 3), поэтому E2E — после неё.

**Сделано:**
- `shell/src-tauri/src/file_ops.rs` (новый): `Request {kind, id, destination, items:[{path, size, mtime}]}` → `Report
  {done, renamed, failed, skipped, cancelled, index, items}`, по элементу на файл (`from`, `to`, `renamed`, `ok`,
  `error`, `index`/`index_error`).
- Preflight: `symlink_metadata` (ссылка — не файл, папка — не файл), размер и дата из выдачи (дата с допуском 1 s);
  несовпадение — `skipped` с понятной причиной, а не `failed`. Папка назначения проверяется до начала операции.
- Авто-переименование: `photo.jpg` → `photo (2).jpg`, `(3)`… до 9999; имена сравниваются без учёта регистра
  (read_dir, как Windows); на гонку — до 20 повторов по `ERROR_ALREADY_EXISTS`. Move в ту же папку — `skipped`
  («it's already in that folder»), copy в ту же папку даёт копию с номером.
- `MoveFileExW` + `MOVEFILE_COPY_ALLOWED`, без `MOVEFILE_REPLACE_EXISTING`; пути — `\`, wide.
- `CopyFileExW` + `COPY_FILE_FAIL_IF_EXISTS`; прогресс — callback, событие `file-op` не чаще 100 ms, `PROGRESS_CANCEL`
  по флагу; недокопированный destination удаляется.
- Корзина: `SHFileOperationW` (`FO_DELETE`, `FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI`,
  список с двойным NUL); проверяются и код возврата, и `fAnyOperationsAborted`; системное предупреждение о слишком
  большом файле не подавляется.
- Индекс: после move — `file_op` с `inode`/`device` из `GetFileInformationByHandle` (те же значения, что считает
  crawler: `nFileIndexHigh<<32|Low`, `dwVolumeSerialNumber`), `size`/`mtime` нового места; после delete — `from`;
  copy индекс не трогает. Если индекс не ответил, `index:false` в отчёте, а команда зовёт `run_index` (repair).
- `main.rs`: команды `file_operation` (в `spawn_blocking`, `block_on` внутри — проверено тестом), `cancel_file_operation`,
  `pick_destination_folder` (тот же `tauri-plugin-dialog`); события `file-op` (`start`/`progress`/`done`) идут в окно
  `main`. В `Cargo.toml` — фича `Win32_Storage_FileSystem`.
- Не-Windows: модуль собирается, но операция возвращает «Windows only for now»; окно её там не предлагает (Phase 3).
- Тесты (11 в `file_ops`): свободное имя; занятое → номер (2), (3); регистр; имя без расширения/dotfile/multi-ext;
  изменённый файл не трогается; папка — не файл; два одинаковых имени оба ложатся; move и copy никогда не
  перезаписывают; Корзина на отсутствующем файле не падает; `block_on` из blocking-thread.

**Проверено (Windows, 2026-10-24):** `cargo check --locked` — ok; `cargo test --locked` — 23 теста, все прошли
(включая настоящие `MoveFileExW`/`CopyFileExW` на временных папках).

**Готово когда:** Move/Copy/Delete работают без перезаписи и без потери данных; ошибки отчитываются.  
**Оценка:** 1,5–2 дня.

### Phase 3 — UI

**Статус:** закрыт (2026-10-24). Сделано в `shell/ui`; Rust-команды и события из Phase 2 уже были на месте.

**Сделано:**
- `Media` — отдельный чип на `Ctrl+M`, счётчик — сумма screenshots/images/video/audio; `All` по-прежнему показывает всё, `Ctrl+1…7` не сдвинуты. На не-Windows чип, чекбоксы, панель, меню и шорткаты операций скрыты: файловые операции там пока не работают.
- `state.selected` (Set путей), `anchor`, существующий `selection` как focus/preview. `operable()` — уникальные медиа из `rows()`, поэтому `Select All` берёт весь текущий список, включая off-screen, а не только DOM. Смена запроса/фильтра очищает выбор; сворачивание weaker убирает исчезнувшие пути; документы и код не получают чекбокс, клик по ним снимает выбор.
- Панель над статус-баром: `N selected · size`, `Select All`, `Deselect All`, `Copy Paths`, `Copy To…`, `Move To…`, `Delete` (красная); без выбора — подсказка и неактивные действия; в Documents/Code панель скрыта.
- Мышь и клавиатура: клик — один, Ctrl/Cmd — toggle, Shift — диапазон от anchor; Shift+↑/↓; Insert — toggle и шаг вниз; Ctrl+A — только вне поля поиска, Ctrl+Shift+A — снять (в том числе из поля); Esc — сначала снять выбор, потом big preview, потом запрос; Delete — только вне поля; F5/F6/F8 — Copy To/Move To/Delete; все новые клавиши уважают `event.isComposing`.
- Чекбоксы — кнопки с `aria-checked`, у них свой `mousedown`, чтобы не начинался drag; grid — `listbox` + `aria-multiselectable`, ячейки — `option` + `aria-selected`; видимый focus ring, Tab по кнопкам листа.
- Move/Copy: системный выбор папки → лист с операцией, числом файлов, размером и путём; Delete — всегда `Move to Recycle Bin` с размером. Прогресс — по файлам и по байтам для Copy, Cancel между файлами и по чанкам; лист прогресса появляется только если работа идёт дольше 300 ms, так что быстрый файл не мигает окном. Отчёта-модалки с одной кнопкой `Done` нет: после операции лист закрывается сам, а переименованные имена, файлы, которые не ушли, и repair-sync появляются строкой под результатами; чистая операция оставляет только строку в статус-баре. Следующая операция убирает отчёт, поиск его оставляет.
- После Move/Delete строки уходят из выдачи без прыжка scroll; провалившиеся и skipped остаются выбранными, чтобы повторить; Copy выделение очищает и строк не добавляет; счётчики чипов пересчитываются из оставшихся strong-результатов.
- Правая кнопка по выбранному работает со всем выделением, по невыбранному — сначала выбирает его; пункты Open, Show in Explorer, Copy, Copy Path(s), Move To…, Copy To…, Delete (Recycle Bin). Сайдбар: один выбранный — preview, несколько — сводка (count, size, первые 30 имён); аудио останавливается при смене выбора.

**Отличия от плана:**
- Отчёт не лист с `Done`: кнопка ничего не меняла, только требовала клик. Он живёт строкой под результатами и не блокирует окно; у чистой операции отчёта нет вовсе.
- Большая партия не разворачивается вся сразу: видны 8 строк и `Show all N`, полный список строится по клику. Это отвечает на 5 000 перемещённых файлов, 500 из которых переименованы: в шапке есть число, а каждое новое имя можно посмотреть и сверить с папкой назначения.
- Число переименований заранее не считается: папку назначения читает Rust только во время операции; лист говорит, что ничего не перезаписывается, а отчёт показывает новые имена.
- После Copy статус — `Copied N files` без `searchable after indexing`: окно не знает, лежит ли папка внутри области поиска с учётом skipped-папок, и не хочет обещать лишнего.

**Проверено (Windows, 2026-10-24):**
- `node --check` для `window.js`, `results.js`, `panel.js`, `setup.js` — ok.
- `cargo build --locked`, `cargo test --locked` — 24 теста, все прошли (включая настоящие `MoveFileExW`/`CopyFileExW`).
- E2E через WebView2 DevTools на `.build/phase3-e2e/search`: поиск `zorblat` → 3 изображения, `Media 3`; Select All → 3 selected · 53 KB, 3 чекбокса, `aria-multiselectable=true`; Ctrl+M переключает Media и очищает выбор; Ctrl+A в поле выбор не трогает, Ctrl+Shift+A снимает; Esc снимает выбор; Delete в поле не открывает подтверждение; отчёт без модалки рисует `photo.jpg → photo (2).jpg` и строку про resync, а у чистой операции отчёта нет.
- На реальных файлах: Move в `…/moved` убрал строку из выдачи и обновил индекс, Delete ушёл в Корзину, Copy конфликтующего имени дал `zorblat-copy (2).png` без перезаписи; Space, стрелки и превью не изменились.

**Готово когда:** Select All выбирает весь текущий список; off-screen тоже; Space не изменился; сценарий Move/Delete проходит от начала до конца.  
**Оценка:** 1,5–2 дня.

### Phase 4 — Интеграция и защита скорости

**Статус:** закрыт (2026-10-24).

**Сделано:**
- `watcher.rs` отдаёт наружу пути всплеска (с дедупликацией), а не только сигнал; пустой всплеск — система потеряла события, значит нужен полный sync. `main.rs` решает по ним: `echo::ours(paths)` — подавить, иначе частичный sync этих папок.
- `Rules.changeMayMatter` портирована в `watcher.rs`: папки и читаемые crawl типы (документы, PDF, изображения, аудио, видео) важны; `.log`, `.tmp`, `.json` и прочие неиндексируемые файлы — нет; исключённые в Settings расширения тоже отсекаются. Пакеты (`.app`, `.photoslibrary` и т.п.) не считаются папками и не будят индексатор.
- Path-aware sync: `index-service` принимает `sync [JSON paths]` и `sync code [JSON paths]`. `Indexer.sync(roots, changedPaths:)` сводит события к папкам (файл → его папка, папка → поддерево, вложенные scopes схлопываются), сверяет только их, а `Crawler.crawl(chosenRoots:)` сохраняет границы выбранных папок, project/dataset-правила и выбранные подпапки внутри исключений. `FolderSelection.crawlPath` резолвит и удалённые пути через ближайшего живого предка.
- Rust `IndexService` получил `sync_paths`/`sync_code_paths`: команда уходит уже запущенному helper'у, а если его нет — он стартует и делает полный sync (частичный запрос не теряется). JSON-пути идут с `/`, как их хранит индекс.
- `echo.rs`: move отмечает оба конца, delete — файл, на 15 s. **На Windows родительские папки больше не отмечаются**: `ReadDirectoryChangesW` называет сам файл, и заметка папки глотала бы чужое изменение в ней. На Mac родительские папки остаются: FSEvents сообщает папку. Copy не отмечается никогда. `file_ops` ставит заметку сразу после успешной файловой операции, **до** обновления индекса: иначе волна из тысяч файлов успевала бы вызвать sync раньше заметок и отобрать у move векторы. Если индекс не ответил, заметка всё равно не мешает repair-sync, который запускает `file_operation`.
- Большая партия не перезагружает векторы на каждом файле: `file_op` идёт с `reload:false`, а после цикла `file_ops` делает один `reload` (если что-то изменилось). Иначе 5 000 перемещений при загруженных векторах перечитывали бы всю матрицу 5 000 раз. Пути в результатах всё равно читаются из БД, поэтому поиск во время операции не показывает старых строк.
- После move/delete без sync счётчики в статус-баре больше не врут: `refresh_counts` спрашивает у search-service `status` (чтение БД, не индексация). Запрос **не ждёт** helper: он идёт в `spawn`, потому что убитый/стартующий search-service иначе задерживал бы отчёт на секунды, а счётчики — только строка в статусе. Поиск всё равно корректен: `file_op` уже изменил БД синхронно.
- Code watcher получает те же пути и шлёт `sync code [...]`; файловые операции пока только по основному индексу, но обычные изменения кода больше не пересканируют все code-папки.
- Отчёт живёт до следующей операции, а не до следующего поиска. Для больших партий показываются первые 8 строк и кнопка `Show all N`: 5 000 перемещённых и 500 переименованных не превращаются в простыню, но каждое новое имя доступно по клику. В шапке — `Moved 5,000 files · 500 renamed`, под списком — пояснение, что имя было занято и DigUp добавил номер.

**Проверено (Windows, 2026-10-24):**
- `cargo clippy --locked --all-targets` — без предупреждений.
- `cargo test --locked` — 37 тестов, все прошли (включая `change_may_matter`, точные Windows-заметки echo, move/copy пути длиннее 260 символов, пропуск cloud placeholder’а, read-only, занятый файл, Unicode-имена и отказ delete за 260 символами).
- `swift test` — 124 теста в 17 suites: `IncrementalSyncTests` 8/8 (изоляция выбранных папок, move между scopes, удаление, выбранная папка внутри исключения, project/package-правила, dataset, пустой список путей); `CrawlerTests` проходит. Единственный провал в `DigUpKitTests` — `modelFilesComeFromTheHuggingFaceCacheWhenThere`: создание symlink на Windows без привилегии (Win32 1314), к этой правке отношения не имеет. `LlamaRuntimeTests.vectorsMatchTheReference` не запускается без модели — тоже среда, не регрессия.
- E2E helper-протокола на `digup.exe index-service`: два root'а, `sync [A/a.txt]` дал `changed=1`, `added=0`, второй root не обойдён; удаление дало `removed=1`; `sync code [code/a.swift]` дал `changed=1`, соседний `README.md` — `unchanged=1`.
- E2E на `.build/phase4-e2e/root` (76 файлов, 100 векторов):
  - Move `photo-06.png` в `moved/`: ни `Checking folders…`, ни `Indexing`; `files=76`, `segments=176`, `vectors=100` до и после; путь, `inode` и `device` в строке обновлены; старый путь исчез из выдачи.
  - Delete `photo-07.png`: ни одного события sync; `files=75`, `segments=174`, `vectors=99`; статус-бар сразу «75 files indexed».
  - Copy `photo-05.png` в `moved/`: watcher сработал через ~2,1 s, «Indexing 1 of 1» закончился через ~6,1 s; `files=76`, `segments=176`, `vectors=100`; копия нашлась поиском.
  - Copy вне root (`.build/phase4-e2e-outside`): файл создан, но `files`/`segments`/`vectors` не изменились — вне области поиска не индексируется.
  - Move/Delete не добавили ни одного вектора: `vectors` 100→100 и 100→99 (минус удалённый файл), `segments` 176→176 и 176→174.
- Отчёт UI без модалки проверен отдельно: чистая операция — `modal.hidden=true`, `report.hidden=true`, только строка статуса; переименование — строка `photo-03.png → photo-03 (2).png`; ошибка/repair — строка `Not done`; новый поиск отчёт оставляет, новая операция убирает.

**Готово когда:** acceptance criteria по скорости и индексу выполнены.  
**Оценка:** 1 день.

### Phase 5 — Полировка

- Длинные пути (`\\?\`) закрыты для move/copy/identity: `wide()` префиксует абсолютные пути и UNC. `SHFileOperationW` extended path не принимает, поэтому delete за 260 символов теперь отклоняется preflight'ом с понятной ошибкой, а не уходит в shell. Тесты гоняют move/copy за 260 символами и проверяют отказ delete.
- Облачные placeholder’ы закрыты: preflight читает атрибуты и пропускает файл с `FILE_ATTRIBUTE_OFFLINE`/`RECALL_ON_OPEN`/`RECALL_ON_DATA_ACCESS` как «only in the cloud, not downloaded», не гидратируя его.
- Read-only и занятые файлы проверены: read-only move/copy проходят (Windows не считает атрибут запретом на rename), занятый файл даёт `Windows says it's in use, or not allowed`, остаётся на месте и может быть обработан после закрытия.
- Отмена и частичные ошибки уже в отчёте (`cancelled`, `failed`, `skipped`, отчёт по каждому файлу); отдельных правок не требуют.
- Подсказки и документация закрыты: панель называет F5/F6/F8, кнопки несут `aria-keyshortcuts`, README описывает операции и больше не говорит, что DigUp только читает файлы.
- Доступность: базовая разметка уже есть (`listbox`/`option`, `aria-selected`, диалог с фокусом и Tab), нужен отдельный проход по клавиатуре и скринридеру.
- Корзина — штатный инструмент Windows, отдельная автоматизация restore не нужна: пользователь восстанавливает штатно, событие создания файла ловит watcher, а если DigUp был выключен — полный sync при следующем запуске. Строка была удалена из индекса (`file_op delete`), поэтому при восстановлении файл **заново эмбеддится**: это ожидаемая цена delete, а не потеря. Отдельный тест restore+sync остаётся ручным E2E.
- Счётчики после операции обновляются fire-and-forget (см. Phase 4); отчёт — до следующей операции.

**Готово когда:** edge cases дают понятный отчёт, нет молчаливых потерь.  
**Оценка:** 0,5–1 день.

**Итого:** 6–8 focused days на ядро; +0,5–1 день на проверку/правки по issues (#144, #130, #142, #101) — **7–9 дней**. Вертикальный срез Move/Delete + UI — 4–5 дней. Объём — примерно 900–1 200 строк.

---

## 8. Acceptance criteria

- [x] Move внутри области поиска: путь обновлён, векторы не пересчитаны, старых строк нет.
- [ ] Move между томами обновляет `inode`/`device`; следующий sync не переэмбеддит файл.
- [ ] Move наружу: запись удалена, файл больше не находится поиском.
- [ ] Delete: запись, FTS и сегменты удалены; после восстановления из Корзины и sync файл снова находится.
- [x] Move/Delete не вызывают `scanning`/`sync` от собственного изменения и не нагружают индексатор.
- [x] Copy не пишет в индекс; внутри индексируемой папки копия появляется после обычной индексации, снаружи — нет.
- [x] Конфликт имён: никогда не перезаписываем; `photo.jpg` → `photo (2).jpg`; финальные имена в отчёте.
- [x] Select All выбирает весь текущий список медиа, включая off-screen; счётчик и размер правильные.
- [x] Смена запроса/фильтра очищает выделение; свёрнутые weaker не выбираются.
- [x] Существующие шорткаты не меняются: Space — preview; Insert — выделение; Ctrl+A — только вне поля поиска; F5/F6/F8 — действия.
- [ ] Ошибки (занятый файл, read-only, другой том, длинный путь, отмена) дают частичный отчёт, а не молчаливую потерю.
- [ ] Move/Copy никогда не перезаписывают существующий файл.
- [x] После Move/Delete старые пути исчезают из выдачи; после Copy выдача не дублируется.
- [ ] `Copy Paths` копирует все выбранные пути, по одному на строку.
- [ ] Unicode-имена (`é`, `日`, турецкие `ı`/`ö`) не ломают Move/Copy/Delete.
- [ ] Move в явно выбранную скрытую подпапку не удаляет запись из индекса (#144).
- [ ] IME-композиция не срабатывает на новые шорткаты; Enter/Escape остаются у IME (#135/#142).
- [x] Swift-тесты на scoped sync и move/delete, Rust-тесты на имена/коллизии/echo/watcher; ручной Windows E2E — частично.
- [ ] Безопасность: подтверждение, Корзина, отчёт; это ответ на возражение upstream из #140.

---

## 9. Тест-план

### Swift

- `IndexStore.move`: путь меняется; сегменты, векторы, FTS name обновляются; число векторов не меняется; переданные `inode`/`device` сохраняются.
- `IndexStore.move` на путь со stale-строкой: не падает на UNIQUE.
- `IndexStore.delete`: запись, FTS и сегменты исчезают; два DELETE — в одной транзакции.
- `file_op move` внутрь области поиска — move; наружу — delete.
- `file_op delete` — delete.
- `Searcher.reload()` после операции не возвращает старые пути.

### Rust

- `unique_destination`: `photo.jpg`, `photo (2).jpg`, регистр, отсутствие расширения, несколько одинаковых имён из разных папок.
- preflight: отсутствующий файл, symlink, изменённый size/mtime.
- no-overwrite: существующий destination.
- copy: частичная ошибка удаляет недокопированный destination.
- trash: успех, ошибка, файл остаётся.

### Ручной Windows E2E

1. Выбрать 10 изображений, Move To в другую индексируемую папку; проверить, что старые строки исчезли, новые находятся поиском, индексатор не запускал полный sync.
2. Move To в папку вне области поиска; файл не находится.
3. Delete в Корзину; восстановить; после sync файл снова находится.
4. Copy To внутрь индексируемой папки; копии появляются после обычной индексации; выдача не дублируется.
5. Copy To наружу; копии не индексируются.
6. Конфликт имён: два `photo.jpg` из разных папок в одну; оба сохранены, второй переименован.
7. Select All после прокрутки: выбраны все медиа текущего списка, не только видимые.
8. Свернуть weaker: скрытые выделенные сняты.
9. Занятый файл, read-only, другой том, длинный путь, отмена. Для другого тома: после move сделать ещё один move и проверить, что переэмбеддинга нет.
10. Пауза индексации во время операции; после resume индекс корректен.
11. Space по-прежнему открывает preview; Insert выделяет; Ctrl+A в поле поиска выделяет текст.
12. Move в явно выбранную скрытую подпапку (#144): файл остаётся в индексе.
13. Unicode-имена и папки: `é`, `日`, `ı`, `ö`.
14. Копирование путей выделения: все пути, по одному на строку.
15. IME (японский/китайский): набор не запускает поиск и не срабатывает на Insert/Ctrl+A/F5/F6/F8.

### Замер скорости

- До/после Move/Delete: нет события `scanning`, нет `sync`, CPU индексатора не растёт.
- Copy внутри области поиска: обычный `scanning`/`sync`, как у любого нового файла; это ожидаемо.
- Время Move/Delete — порядка файловой операции, не обхода папок.
- База для сравнения (2.3): 76 файлов — 0,15 s неизменный sync и 7 s полный проход на CUDA; move/delete 10 файлов
  должны быть < 1 s и без строк индексатора. На CPU те же 76 файлов — 178 s, то есть CPU-замеры без проверки
  `backends` непригодны.

---

## 10. Риски и открытые вопросы

| Риск | mitigation |
|---|---|
| SQLite busy, если index-service пишет одновременно | WAL, `busy_timeout`, повтор; при неудаче repair-sync |
| Watcher-подавление скроет чужое изменение в том же всплеске | подавлять только если все пути всплеска — наши; на Windows заметки точные (файл, без родительской папки), на Mac родитель остаётся из-за FSEvents; иначе — scoped sync |
| Большой файл не помещается в Корзину | не подавлять системное предупреждение; не удалять навсегда молча |
| Другой том при Move | `MOVEFILE_COPY_ALLOWED`; проверить, что source удалён; обновить `inode`/`device`; иначе следующий sync переэмбеддит |
| CUDA backend молча падает в CPU | `ggml-cuda.dll` требует `cublas64_13.dll`/`cudart64_13.dll` из `CUDA/v13.1/bin/x64`; без них `backends: CPU` без ошибки. Подтверждено на этом стенде: запуск из местного Git Bash без `cmd.exe` тоже даёт `backends: CPU` (портится PATH), индекс в 25× медленнее. В установщик класть эти DLL; перед baseline и E2E проверять `backends: CUDA, CPU` в `worker.log` |
| `.build/release` не создался | SwiftPM 6.4 кладёт exe в `.build/out/Products/Release-windows-x86_64`; `package-windows.sh` теперь берёт путь из `swift build -c release --show-bin-path` — проверено сборкой установщика |
| `swift build` удаляет tracked `Package.resolved` | наблюдалось дважды: после сборки файл в статусе ` D` (вероятно, потому что на Windows у пакета нет зависимостей — Sparkle только для macOS); после каждой сборки `git checkout -- Package.resolved`, не коммитить его удаление |
| Замеры из Git Bash дают CPU вместо CUDA | PATH дочернего процесса портится; запускать только через `.bat`/`cmd.exe` и проверять `backends: CUDA, CPU` в `worker.log` перед каждым замером |
| ffmpeg из пути с пробелами | `dav1d.pc` экранирует пробел, configure не находит заголовок; собирать через junction без пробелов (`D:\DigUpBuild`) |
| Git's `link.exe` перехватывает MSVC-линкер | в Git Bash `/usr/bin` идёт раньше MSVC; перед `package-windows.sh` положить `VCToolsInstallDir/bin/Hostx64/x64` в начало PATH |
| CUDA runtime в установщике без notices | закрыто: `notices.py windows` читает EULA и notices cuBLAS из CUDA, чьи DLL положил `package-windows.sh` (`CUDA_RUNTIME_DIR`) |
| `swift test` на Windows: `abnormal(309)` | закрыто для релизной сборки: `Testing.dll`/`XCTest.dll` из `…/Developer/Library/{Testing,XCTest}-6.4.0/usr/bin64`, `Runtimes/6.4.0/usr/bin`, `Vendor/{llama/lib,pdfium/bin,ffmpeg/bin}` в `PATH`; без ffmpeg ImageTests ложно валятся на orientation. Остались только средовые провалы: symlink без привилегии и `ReferenceVectorTests` без модели |
| `std::fs::rename` на Windows перезаписывает | использовать `MoveFileExW` без `MOVEFILE_REPLACE_EXISTING` |
| Длинные пути | `\\?\` или long-path-aware; тест |
| Copy внутри индексируемых папок добавляет работу индексатору | ожидаемо; это обычная индексация новых файлов, не регрессия |
| Нет undo для Move/Copy | явно указать в UI; Delete — Корзина |
| `SHFileOperationW` устарел | при проблемах с per-file отчётом перейти на `IFileOperation` |
| **#140: upstream против delete/move в поиске** | это осознанное расхождение; подтверждение, Корзина, no-overwrite, отчёт; Copy Paths как безопасная альтернатива |
| **#144: скрытая явно выбранная подпапка** | `isCrawled` (`Rules.isWalked`) отличает путь, до которого crawler дойдёт, от `isSearched`; всё равно проверить Settings/`Crawler` тестом до Move |
| `isCrawled` не знает про `Rules.datasetFolders` | move в папку, которая после операции выглядит датасетом, может оставить строку до следующего sync; добавить проверку или тест и честный отчёт |
| **#130: Unicode-пути на Windows** | только wide API; E2E с `é`/`日`/`ı`; не использовать ANSI-функции |
| **#101 не портирован на Windows** | watcher реагирует на любые изменения; подавление эха обязательно; порт `changeMayMatter` — отдельная правка |
| **#142: IME** | новые клавиши не должны срабатывать при `isComposing`; проверить после мержа PR |
| **#145: reload и preview** | вызывать публичный `Searcher.reload`; не полагаться на старую реализацию; rebase при мерже |
| **#110/#114: частичный sync** | не строить параллельный механизм; первый срез — подавление эха; затем `sync <paths>` |
| Mac/Linux | вне первого среза; UI скрывает операции или показывает «Windows only» |

**Блокирующих вопросов для Phase 0–3 нет.** До первого Move в скрытые или датасет-папки нужно зафиксировать поведение #144 и `Rules.datasetFolders`; безопасный fallback — удалить строку и сказать «destination isn't indexed». Остальные спорные места (Copy внутри области поиска, watcher-подавление, размер Корзины) зафиксированы выше и проверяются в Phase 4/5.

---

## 11. Точки в коде

| Файл | Роль |
|---|---|
| `Sources/DigUpKit/IndexStore.swift` | `FileRecord`, `insert`, `move`, `delete`; сюда добавить `file(at:)`/`file(id:)` и транзакцию в `delete` |
| `Sources/DigUpKit/Indexer.swift` | `sync` — полный обход; сюда guard `stillIndexed` в `commit`/извлечении |
| `Sources/DigUpKit/FolderSelection.swift` | `isSearched` — внутри ли путь области поиска; для move destination используем `Rules.isWalked`/`isCrawled` |
| `Sources/DigUpKit/Searcher.swift` | `reload()` — перезагрузка векторов после изменения; `vectorsLoaded` |
| `Sources/digup/SearchService.swift` | JSON-протокол; сюда добавить `file_op` |
| `Sources/digup/main.swift` | команда `search-service`, аргументы |
| `shell/src-tauri/src/helper.rs` | запуск `search-service` и `index-service` |
| `shell/src-tauri/src/main.rs` | Tauri-команды, `invoke_handler`, `ask` whitelist, watcher wiring |
| `shell/src-tauri/src/file_ops.rs` (новый) | preflight, `MoveFileExW`/`CopyFileExW`/`SHFileOperationW`, auto-rename, прогресс |
| `shell/src-tauri/src/watcher.rs` | callback с путями и подавление эха |
| `shell/ui/window.js` | `selected`/`anchor`, клики, клавиатура, панель, модалка, отчёт |
| `shell/ui/window.html` | разметка панели и модалки |
| `shell/ui/window.css` | `checked`, чекбоксы, панель, модалка |
| `Sources/DigUpKit/Rules.swift` | `isWalked` — достижимость пути для crawler; используется в `isCrawled` |
| `shell/ui/results.js` | `bytes()`, `openResult`, `copyResult` |
| `Package.swift` | Windows-сборка движка и `digup` |
| `shell/src-tauri/Cargo.toml` | `windows-sys`; добавить фичу `Win32_Storage_FileSystem` |

---

## 12. Следующий шаг

Phase 0–2 закрыты: сборка CUDA, baseline и smoke-test (2.3), shell с helper и установщик собраны; индекс следует за
move/delete через `file_op` (Phase 1, тесты и сквозная проверка), а Rust-команды уже двигают, копируют и убирают
файлы в Корзину (Phase 2, 11 тестов). До релиза осталось прогнать установщик на чистой машине; лицензия CUDA в
notices уже есть.

Дальше — **Phase 3** (UI): чип Media (`Ctrl+M`), панель действий, `selected`/`anchor`/`focus`, клики и клавиши
(Insert, Ctrl+A, Shift+стрелки, F5/F6/F8), модалка подтверждения, прогресс и отчёт, правая кнопка по выделению,
сайдбар со сводкой. Окно зовёт готовые команды: `file_operation` с
`{kind:"move|copy|delete", id, destination, items:[{path, size, mtime}]}`, `cancel_file_operation {id}`,
`pick_destination_folder {title}`; живые события — `file-op` (`start`/`progress`/`done`) в окне `main`.

Phase 4 закрыт: watcher больше не запускает полный sync на каждое изменение, move/delete подавляются точно, а
path-aware sync пересобирает только затронутые папки. В Phase 5 закрыты длинные пути, read-only, занятые и облачные
файлы, отмена и частичные ошибки уже в отчёте, подсказки и документация обновлены. Остались доступность и ручной
E2E восстановления из Корзины.

Блокирующий вопрос перед первым Move в скрытые/датасет-папки — поведение #144 и `Rules.datasetFolders`; безопасный
fallback (реализован): строка удаляется, а отчёт говорит, что папка назначения не индексируется.
