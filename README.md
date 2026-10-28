<p align="center"><img src="docs/logo.png" width="96" alt=""></p>

<h1 align="center">DigUp Plus</h1>

<p align="center"><b>Describe it. Dig it up.</b><br>
Search the files on your computer by what's in them, with Google DeepMind's
<a href="https://huggingface.co/google/embeddinggemma-2">EmbeddingGemma 2</a>.</p>

<p align="center">A Windows-focused fork of <a href="https://github.com/ARahim3/DigUp">DigUp</a>: paged results, file
operations in the search window (move, copy, delete), index and model folders of your own, and portable Windows
builds. <a href="#this-fork-vs-upstream">What it changes over upstream</a>.</p>

<p align="center"><a href="https://github.com/Sucotasch/DigUpPlus/releases"><b>Download for Windows</b></a><br>
<sub>Free and open source. Everything runs on your computer.</sub></p>

<p align="center">
<a href="https://github.com/Sucotasch/DigUpPlus/releases"><img src="https://img.shields.io/github/v/release/Sucotasch/DigUpPlus?style=flat-square" alt="Latest release"></a>
<a href="LICENSE"><img src="https://img.shields.io/github/license/Sucotasch/DigUpPlus?style=flat-square" alt="MIT license"></a>
</p>

<p align="center"><img src="docs/demo.gif" width="800" alt="DigUp's search panel on a Mac, on Windows and on Linux: a zebra in a video, a podcast minute, a clause in a lease, a dog on the beach in English, Bengali and Arabic, and code that opens in Zed."></p>

EmbeddingGemma 2 is the multimodal embedding model Google DeepMind released in October 2026. It puts text, images,
audio and video into one shared space: a photo of a dog on a beach lands next to the words "a dog on the beach", and a
recording of someone talking about sleep lands next to "where they talk about sleep". It understands more than 100
languages, it's open (Apache 2.0), and at 740M parameters it's small enough to run on a laptop.

DigUp brings that to the files on your computer. Pick the folders to search, and it
reads the pictures, PDFs, documents, recordings and videos in them. Then describe what you're looking for, in your own
words, and it finds the match, whatever kind of file it is.

## This fork vs upstream

The engine is the same Swift code as upstream's: one `DigUpKit` behind the app shell, the same model, the same index
format, the same privacy rules. The macOS SwiftUI app stays in the tree as the reference behavior but isn't built
here. What this fork changes is what surrounds the engine:

- **Windows only, portable.** Upstream ships a macOS app with an installer and an updater; this fork builds a portable
  Windows folder — `DigUpPlus.exe` beside `helper\` — that runs from anywhere with no install step, no admin rights
  and no updater. Releases are cut by hand; GitHub Actions is off.
- **File operations.** The big results window can move, copy and delete the files it shows. Delete goes to the
  Recycle Bin; paths it refuses (too long) get a separate, explicitly confirmed permanent delete. A taken name becomes
  `photo (2).jpg` instead of an overwrite, and the index follows every change without reading the files again.
- **The index and the model live where you point them.** Settings → Storage: use an index you already have in place
  (nothing is copied, and it is checked to open before it is chosen), move one to another disk, or point at model
  files you already downloaded.
- **A first run opens Settings, not a wizard.** Nothing has to be chosen before DigUp is usable, and nothing
  downloads by itself — the model asks first.
- **Paged results, no cap.** The window opens on the best 120 and appends the rest 300 at a time as you scroll or
  click **Show 300 more**, and there is no limit on how far it goes: the whole ranking of what is indexed stays
  reachable. A failed page turns the button into **Retry** without losing what's already loaded.
- **Images: the whole ranking, in cosine layers.** The Images filter drops the mixed score for the **best segment's
  cosine** — the best-matching region of a picture, moment of a video or stretch of audio — and pages the entire
  ranking instead of stopping at a cap. It is cut at fixed thresholds: cosine 0.648 and up is shown at once;
  0.600–0.648 folds under *N weaker matches* — close enough to be worth a look, not close enough to open unasked;
  anything below 0.600 folds away as *N hidden images* until you open it, which keeps the irrelevant tail out of
  sight without ever losing it. Files that matched by exact words stay on top whatever their cosine, and a query
  whose own scale sits low gets both thresholds dropped under its best image, so its results aren't all hidden.
  Scroll on its own only fetches the visible layer — the weaker tail waits for its fold or for the **Show 300 more**
  button, so a click never pulls in rows you can't see.
- **A keyboard-first results window.** Tab switches between the search field and the results, Del deletes the file
  under the cursor (the ticked ones, when some are ticked), Enter confirms the delete sheet, and the view stays on the
  file it was on across operations and index updates.

## What you can find

- "zebra in a video" opens the clip at the moment the zebra shows up
- "where they talk about sleep" jumps to that minute of a podcast
- "the clause about pets in the lease" shows the PDF page
- "a dog on the beach" finds the photo, and "payment declined error" the screenshot
- a search in English can find a note written in Bengali, Arabic or any of the model's 100+ languages, and the other
  way around
- "code: retry a failed call with backoff" finds the function in your projects, if you turn on code search

Exact words count too. Invoice numbers, error codes and names are matched as written, in file names and document text,
so a search gets both the meaning and the exact string. On a Mac that includes the text inside screenshots.

## Private

Nothing you index leaves your computer. DigUp Plus goes online only to download the model once. No update check, no
account, no telemetry.

## Install

**Windows.** Download `DigUpPlus-x.y.z-windows-x64.zip` from
[Releases](https://github.com/Sucotasch/DigUpPlus/releases), unpack it anywhere and run `DigUpPlus.exe`. Nothing is
installed, so no admin rights are needed: the helper, llama.cpp's libraries, PDFium, the Swift and C++ runtimes and
ffmpeg sit in the `helper` folder beside the app. Windows may say it doesn't recognize the app, because it isn't
signed yet: choose More info, then Run anyway. It needs 64-bit Windows 10 or 11.

The zip is portable: settings, logs, the index and the model live in `data` inside the folder you unpacked (Settings →
Storage shows it), so a new zip can replace the old one without re-indexing. To keep that folder somewhere else, launch
`DigUpPlus.exe --data <folder>` (or set `DIGUP_DATA`).

1. Open DigUp Plus. The first start opens **Settings** and stops there: nothing downloads and nothing is indexed
   until you choose.
2. **Settings → Storage** is where the model and the index live. The model is two files, 865 MB, downloaded once:
   pick the folder (**Change Folder…**; `data` beside the app by default) and press **Download Model…** — it asks
   before it starts and shows the progress — or **Use Model Files…** to point at the two GGUF files you already
   have. The same tab holds the index: **Use Existing Index…** adopts an index you already have where it is (nothing
   is copied, and it is checked to open first), **Move Index Folder…** moves it to another disk; by default the
   index lives in `data`.
3. **Settings → Folders** is what gets indexed: **Add Folders…** whole folders, or open one and tick only some of
   its subfolders. Each choice shows how long the first pass will take, measured on your computer. Once the model is
   in place, the first pass starts on its own; after that a new file is searchable a few seconds after it lands.
4. Press **Ctrl+Shift+Space** in any app and describe what you're looking for.

DigUp Plus doesn't update itself. Settings → General shows the version and opens the releases page; a new zip replaces
the old folder.

**Updating.** For a first install take the full package. When a release's notes say the helper is unchanged, its
small `-shell-only.zip` carries just `DigUpPlus.exe`: close DigUp Plus, drop it into your unpacked folder beside
`helper\`, and you're done — nothing else moves, and your `data` folder stays as it is. The exe refuses to run with a
helper from a different generation, so a mismatch can't happen silently; if it ever asks for the full package, take
the full package.

## Using it

| Windows and Linux | What it does |
|---|---|
| Ctrl+Shift+Space | Opens the search panel from any app (you can change it) |
| ↓ ↑ | Moves through the results |
| Enter | Opens the result: PDFs at the page, recordings and videos at the moment, code in your editor at the line |
| Space | A big preview, at the page or moment (once you've moved to a result) |
| Ctrl+Enter | Shows it in Explorer or your file manager |
| ⌘C | Ctrl+C (in the window) | Copies the file, to paste anywhere |
| ⌘O | Ctrl+O | The same search in a bigger window, with a picture grid and filters |
| — | Insert / Ctrl+A | Selects results in the big window to copy, move or delete (Windows; Ctrl+A outside the search field) |
| — | Tab | Switches the keyboard between the search field and the results (big window) |
| — | Del | Deletes the file under the cursor, or the ticked ones (Windows); Enter confirms the Recycle Bin sheet |
| — | Ctrl+1…6 | The big window's views: Images, Screenshots, Documents, Video, Audio, All files |
| — | F5 / F6 / F8 | Copy To… / Move To… / Delete the selected files (Windows) |

DigUp Plus lives in the tray, where you can see how indexing is going,
pause it, or open Settings. Opening it from the Start menu or your app list shows the search window, which opens on
the **Images** view — the fork's home ground: the whole picture ranking in cosine layers, paged as deep as it goes.
The **All files** view (the last chip, Ctrl+6) ranks everything by the mixed score and is where a query that mixes
kinds — a receipt, a lease page, a photo — shows its best across all of them.

On Windows, the big window can move, copy and delete the pictures, screenshots, video and audio a search shows:
select them and use the bar under the results or the right-click menu. A name that's taken gets a number
(`photo (2).jpg`) instead of overwriting a file; Delete goes to the Recycle Bin, and the confirmation sheet opens with
the keyboard on the delete itself (Enter confirms, Escape keeps the files). If a path is too long for the Recycle
Bin, DigUp lists the affected folders and offers a separate permanent delete only after you confirm that it cannot be
undone — there Enter keeps the files, and only the click deletes. Moving or deleting updates the index without
reading the files again, and a copy that lands in a folder you search is indexed like any new file.

A few things differ by system:

- On a Mac, a video opens at its moment in QuickTime Player (macOS asks once whether DigUp may control it) or IINA,
  and a PDF opens in Preview with your words in its search. On Windows and Linux, PDFs open at the page in your PDF
  viewer if it's one that can be told a page (Edge, Chrome, Firefox, Acrobat, SumatraPDF, Evince, Okular), and
  recordings and videos at the moment in VLC, mpv or MPC-HC. Other apps open the file from the start; the big
  preview plays it from the moment either way.
- On GNOME and KDE under Wayland, which don't let apps set their own shortcuts, DigUp adds Ctrl+Shift+Space to the
  desktop's keyboard settings during setup, and you change it there. On other Wayland desktops, Settings shows the
  command to give a shortcut of your own.
- On GNOME, the tray icon needs the AppIndicator extension, which Ubuntu includes. Without it, the window's ⋯ menu has
  the same items.

## What it reads

Only the folders you choose:

- pictures and screenshots (JPEG, PNG, HEIC, WebP and more), by what they show, and on a Mac screenshots by their text
  too
- PDFs, page by page, scans included. A long one gets its first 30 pages right away and the rest once everything
  else is in (up to 5,000 pages a file)
- documents: txt, md, typ, rtf, docx, odt, html, and on a Mac doc, long ones the same way. Text that's mostly numbers,
  like a data export, is matched by its exact text only
- spreadsheets and presentations: xlsx, ods, pptx and odp, read for their text: each sheet's name and rows, each
  slide's text and speaker notes
- audio of any length: the model listens to 30 seconds at a time, so DigUp takes overlapping 30-second windows and a
  match points to its moment
- video of any length: a frame every few seconds, one per shot, plus the soundtrack the same way as audio

Outside the folders you pick for code search, it never reads code, and inside a code project (a folder with `.git`,
`package.json` and the like) it reads only screenshots. It never reads keys, certificates, password files, hidden
folders like `.ssh`, apps or caches. It skips folders that look like datasets, and never downloads files that only
live in the cloud (iCloud on a Mac, OneDrive on Windows). You can skip more folders or file types in Settings. DigUp
never uploads your files. On a Mac it only reads them; on Windows it changes, moves or deletes only the results you
choose in the big window. Delete goes to the Recycle Bin; a path the Recycle Bin can't take gets a separate permanent
delete only after an explicit confirmation.

The first pass happens once. On a Mac it takes a few minutes for a few hundred files and can take an hour for a big
Downloads folder. On Windows and Linux, DigUp runs the model on the graphics card where that's faster and on the
processor otherwise; on a processor alone a picture takes a few seconds (about five on a 2017 laptop's Core i5), so a
large photo library can take hours, which is why setup shows the time first. After that, a new file is searchable a
few seconds after it lands.

## Code

EmbeddingGemma 2 reads code too, so DigUp can search your projects. Code search is off until you turn it on, in
Settings → Code or during setup, for the folders you pick. Each repo shows how long reading it takes and how much room
its index needs. Then start a search with `code:` and say what the code does, or type a name that's in it:

- "code: retry a failed call with backoff" finds the function, whatever it's called
- "code: refreshAccessToken" goes to the line that defines it, not the places that call it
- "code: plot how many examples each class has" finds the notebook cell

Results open in your editor at the line: Zed, VS Code, Cursor, Sublime Text, JetBrains IDEs and a few more, Xcode on
a Mac and Notepad++ on Windows (pick one in Settings). Code has an index of its own and never shows up in other
searches. DigUp reads source files, notebooks (without their outputs) and a repo's own docs. It skips what git
ignores, vendored and generated code, data files, and anything that looks like a key or a password. While you're
working on code, the index catches up a minute after things go quiet.

On a Mac, reading code takes about 20 seconds for a small app, and about 20 minutes and 130 MB of index for all of
llama.cpp.

## How it works

```
your folders ─► readers that turn files into pictures, text and sound
             ─► EmbeddingGemma 2 on llama.cpp, in a helper process that quits when it's done
             ─► one SQLite file: a vector for every picture, page, passage, video frame and 30 s of sound,
                plus a keyword index (code search keeps a second one, for code)
your words   ─► the same model's text part (~250 MB, only while you search) ─► nearest vectors + exact words
```

DigUp runs ggml-org's 8-bit build of EmbeddingGemma 2 through [llama.cpp](https://github.com/ggml-org/llama.cpp): on
the Mac's GPU with Metal, and on Windows and Linux on the GPU with Vulkan when a quick check at setup finds it faster
than the processor and getting the same results. Every vector lives in the same 768-dimensional space, which is why
one query can rank a video frame against a PDF page. A search embeds your words in milliseconds and compares them with
everything in the index. Exact-word matches get a boost, because no embedding holds an invoice number exactly.

The search engine is the same Swift code on every system. On a Mac it reads files with macOS's own frameworks (ImageIO,
PDFKit, AVFoundation, and Vision for the text in screenshots) and the app is SwiftUI. On Windows and Linux it reads
them with PDFium, ffmpeg, stb and miniz, and the app around it is built with [Tauri](https://tauri.app).

The index remembers which model version made its vectors. An update that leaves the vectors unchanged keeps your
index, and a test checks every llama.cpp update against stored reference vectors before it ships.

## Build from source

```sh
git clone https://github.com/Sucotasch/DigUpPlus && cd DigUpPlus
git switch file-operations        # this fork's default branch
scripts/fetch-vendor.sh           # SQLite, stb, miniz, PDFium (hash-pinned)
scripts/build-llama.sh            # llama.cpp at a pinned tag, with a one-line patch (a few minutes)
scripts/build-ffmpeg.sh           # in MSYS2's MINGW64 shell
swift build -c release && swift test           # tests: python .build/swift_test.py
scripts/package-portable.sh       # → shell/src-tauri/target/release/DigUpPlus.exe (+ helper, portable.txt)
```

You'll need Swift 6.4, Rust and Node, and Visual Studio's C++ build tools, with the commands run in Git for Windows'
bash inside a developer environment. `docs/development-guide.md` has the Windows steps in full, including the tests
and the live runs. This fork's releases are the portable zip, cut by hand: upstream's `scripts/package-windows.sh`
(NSIS installer) and `scripts/package-linux.sh` (.deb and AppImage) are not part of it, and [the packages
workflow](.github/workflows/packages.yml) that documents what each system needs is disabled here.

There's also a command-line tool, `.build/release/digup`, that indexes, searches and runs the evals from the terminal
(`digup --help`). `scripts/make-testbed.sh` builds a folder of test files to try it on on a Mac (it needs ffmpeg), and
`evals/` holds the queries ranking changes are measured with.

## FAQ

**How is this different from Spotlight or Windows Search?** They find files by their names, text and metadata. DigUp
also finds them by what they show or say, and takes you to the page or moment. They work fine side by side.

**What's different on Windows and Linux?** The search is the same: the same model, the same results. Two things are
missing for now. The text inside screenshots isn't read yet, so a screenshot is found by what it shows rather than by
an exact code typed into the search. And doc files from old Word versions, RAW photos and SVG pictures are only read
on a Mac.

**Why is the download 865 MB?** That's the model: the text part (310 MB) and the image and audio encoders (555 MB).
The app itself is about 20 MB on a Mac. This fork's portable zip is about 470 MB, mostly the CUDA runtime the helper
carries so the model runs on NVIDIA cards; upstream's smaller packages use the GPU through Metal or Vulkan.

**Will it slow my computer down?** Only the first pass is heavy. After that it reads new files only. The model runs in
a separate process that quits when it's done, and the search side only loads while you're searching.

**Which languages?** The model knows more than 100, and DigUp searches in all of them, across languages too. The evals
cover only English, Bengali and Arabic because those are the languages of the files DigUp was tested on, and Google
notes that the model isn't equally strong in every language. Two limits apply to exact words, not to meaning: the
text inside screenshots is read only on a Mac, by Apple's text recognition, which knows about 25 languages, and
languages written without spaces, like Chinese, Japanese and Thai, aren't split into words.

**How do I remove it?**
Quit DigUp Plus from its tray menu and delete the folder you unpacked — that is everything: settings, logs, the
index, the model and even the webview's own files live in `data` inside it. Nothing is written elsewhere, except the
open-at-login entry, which is a Run key in the registry — turn that off in Settings → General first if you used it.

## Contributing

Issues and pull requests are welcome — please read [CONTRIBUTING.md](CONTRIBUTING.md) first: it asks for an issue
before the code, and for a change that moves what search returns, before/after numbers from the queries in `evals/`,
so the ranking can be seen to hold.

## Thanks

This fork builds on [DigUp](https://github.com/ARahim3/DigUp) by
[ARahim3](https://github.com/ARahim3) (MIT): the engine, the design and most of the code come from the original
project — the changes this fork makes on top of it are listed
[above](#this-fork-vs-upstream), and the copyright notice in [LICENSE](LICENSE) is the original's.

DigUp is built on [EmbeddingGemma 2](https://huggingface.co/google/embeddinggemma-2) by Google DeepMind (Apache 2.0)
and [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT). The app is built with [Tauri](https://tauri.app) (MIT or
Apache 2.0) and reads
files with [PDFium](https://pdfium.googlesource.com/pdfium/) (BSD), [FFmpeg](https://ffmpeg.org) (LGPL),
[stb](https://github.com/nothings/stb) (public domain) and [miniz](https://github.com/richgel999/miniz) (MIT). Their
license notices ship with the app.

DigUp isn't affiliated with Google, Apple or Microsoft. Gemma is a trademark of Google LLC; Mac and Spotlight are
trademarks of Apple Inc.; Windows is a trademark of Microsoft Corporation.

## License

MIT — the same license as the original DigUp. [LICENSE](LICENSE) carries the upstream copyright notice (© 2026
Abdur Rahim), which this fork's changes inherit; the third-party notices ship with the app
(`THIRD-PARTY-NOTICES.txt`).
