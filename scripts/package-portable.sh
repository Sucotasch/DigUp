#!/bin/bash
# Builds DigUp Plus's portable Windows folder: the Tauri shell beside its helper (llama.cpp with the CUDA runtime it
# needs, PDFium, the Swift and C++ runtimes, ffmpeg), zipped so it runs from any folder with no install step. This fork's
# builds come from here: a portable folder is quicker to hand out and to replace while the app is still changing shape.
# Settings, the index and the model still live in the app's own folders (%APPDATA% and %LOCALAPPDATA% under
# com.sucotasch.digupplus); "portable" means nothing is installed and nothing else is written.
# Run it in Git for Windows' bash inside a Visual Studio developer environment, after scripts/fetch-vendor.sh,
# scripts/build-llama.sh and scripts/build-ffmpeg.sh (that one in MSYS2's MINGW64 shell). Needs Swift, Rust, Python and
# Node (for Tauri's CLI):
#   scripts/package-portable.sh      → dist/DigUpPlus-<version>-windows-x64.zip
set -euo pipefail
cd "$(dirname "$0")/.."
[[ -x Vendor/ffmpeg/bin/ffmpeg.exe ]] || { echo "no Vendor/ffmpeg/bin/ffmpeg.exe: run scripts/build-ffmpeg.sh in MSYS2" >&2; exit 1; }
TAURI_CLI=2.12.2

version=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' shell/src-tauri/tauri.conf.json | head -1)
product=$(sed -n 's/.*"productName": *"\([^"]*\)".*/\1/p' shell/src-tauri/tauri.conf.json | head -1)
[[ -n "$version" && -n "$product" ]] || { echo "no version or productName in shell/src-tauri/tauri.conf.json" >&2; exit 1; }

swift build -c release --product digup
# SwiftPM 6.4 can't make the .build/release link on Windows (I/O error 512), so ask it where the exe is.
bin=$(swift build -c release --show-bin-path)

# What goes beside the shell, in helper\ ("DigUp Plus.exe" and "digup.exe" can't share a folder): the helper finds the
# DLLs, llama.cpp's backends and ffmpeg in its own folder. The C++ runtime goes with them, so nothing needs installing
# first; the shell itself links its C runtime statically (.cargo/config.toml).
swift_root=$(cd "$(dirname "$(command -v swift)")/../../../.." && pwd)   # …/Swift from …/Swift/Toolchains/*/usr/bin
swift_runtime=$(ls -d "$swift_root"/Runtimes/*/usr/bin | tail -1)
crt=$(ls -d "$(cygpath "$VCToolsRedistDir")"x64/Microsoft.VC*.CRT | tail -1)   # set by the developer environment
helper=shell/src-tauri/helper
rm -rf "$helper"
mkdir -p "$helper"
cp "$bin/digup.exe" Vendor/llama/lib/*.dll Vendor/pdfium/bin/pdfium.dll "$helper/"
# ggml-cuda.dll also needs the CUDA runtime, which isn't in Vendor/llama/lib. Without it the helper finds the CUDA
# backend missing and silently runs on the processor, so a package that can't have it isn't made. CUDA_PATH (set by the
# CUDA installer) or the newest CUDA in its default folder says where it is.
cuda_root="${CUDA_PATH:-C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA}"   # already a Windows path: not through cygpath
cuda=$(ls -d "$cuda_root"/bin/x64 2>/dev/null || ls -d "$cuda_root"/*/bin/x64 2>/dev/null | sort -V | tail -1 || true)
[[ -n "$cuda" ]] || { echo "no CUDA runtime in $cuda_root/bin/x64: set CUDA_PATH (the helper would run on the processor)" >&2; exit 1; }
cp "$cuda"/cublas64_*.dll "$cuda"/cublasLt64_*.dll "$cuda"/cudart64_*.dll "$helper/"
cp "$swift_runtime"/*.dll "$helper/"
cp "$crt"/*.dll "$helper/"   # over the Swift runtime's copies of the same: the newer C++ runtime
cp Vendor/ffmpeg/bin/ffmpeg.exe Vendor/ffmpeg/bin/ffprobe.exe "$helper/"   # the small static build, no DLLs

# The licenses of everything the folder carries, beside the app. (python3 can be the Store's stub.)
python=python3
python3 -c '' 2> /dev/null || python=python
CUDA_RUNTIME_DIR="$cuda" "$python" scripts/notices.py windows shell/src-tauri/THIRD-PARTY-NOTICES.txt

# The shell alone: no installer, no bundle. Its helper folder is a sibling in the zip, not a bundle resource.
(cd shell/src-tauri && npx --yes "@tauri-apps/cli@$TAURI_CLI" build --no-bundle)

# cargo writes digup-shell.exe; Tauri renames it to productName, which is what a build with bundles leaves behind.
built=shell/src-tauri/target/release/digup-shell.exe
[[ -f "$built" ]] || built="shell/src-tauri/target/release/$product.exe"
[[ -f "$built" ]] || { echo "no shell binary in shell/src-tauri/target/release" >&2; exit 1; }
folder="dist/DigUpPlus-$version-windows-x64"
rm -rf "dist"/*.zip "$folder"
mkdir -p "$folder"
cp "$built" "$folder/$product.exe"
cp -r "$helper" "$folder/helper"
cp shell/src-tauri/THIRD-PARTY-NOTICES.txt "$folder/"
# Says this copy is portable: the shell keeps settings, logs, index and model in `data` here (options.rs) instead of
# %APPDATA% and %LOCALAPPDATA%. `--data <folder>` or DIGUP_DATA points them anywhere else.
printf 'DigUp Plus keeps its settings, logs, index and model in the data folder beside this file.\n' > "$folder/portable.txt"
(cd dist && powershell -NoProfile -Command "Compress-Archive -Path 'DigUpPlus-$version-windows-x64' -DestinationPath 'DigUpPlus-$version-windows-x64.zip' -Force")
ls -la dist/
