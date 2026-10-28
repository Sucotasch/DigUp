#!/bin/bash
# Builds DigUp Plus: the Tauri shell beside its helper (llama.cpp with the CUDA runtime it needs, PDFium, the Swift and
# C++ runtimes, ffmpeg) in shell/src-tauri/target/release, the one build place, with a portable.txt that keeps its
# settings, logs, index and model in the `data` folder there (options.rs) instead of %APPDATA% and %LOCALAPPDATA% under
# com.sucotasch.digupplus. Nothing is installed and nothing else is written; the folder runs from anywhere, and a copy of
# it runs from anywhere too.
# Run it in Git for Windows' bash inside a Visual Studio developer environment, after scripts/fetch-vendor.sh,
# scripts/build-llama.sh and scripts/build-ffmpeg.sh (that one in MSYS2's MINGW64 shell). Needs Swift, Rust, Python and
# Node (for Tauri's CLI):
#   scripts/package-portable.sh      → shell/src-tauri/target/release/DigUpPlus.exe (with helper\ and portable.txt)
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

# What goes beside the shell, in helper\: the helper finds the DLLs, llama.cpp's backends and ffmpeg in its own folder
# (the helper used to share the shell's folder, where its digup.exe would have collided with the shell's older name
# DigUp.exe: Windows doesn't tell them apart). The C++ runtime goes with them, so nothing needs installing
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

# The shell alone: no installer, no bundle. Its helper folder is a sibling in the folder, not a bundle resource.
(cd shell/src-tauri && npx --yes "@tauri-apps/cli@$TAURI_CLI" build --no-bundle)

# Everything lands in target/release, the one build place: the shell beside its helper, the notices and portable.txt.
release=shell/src-tauri/target/release
# cargo writes digup-shell.exe; Tauri renames it to mainBinaryName ("DigUpPlus"). Both names are looked for, so the
# step works either way.
exe=$release/DigUpPlus.exe
built=$release/digup-shell.exe
[[ -f "$built" ]] || built="$exe"
[[ -f "$built" ]] || { echo "no shell binary in $release" >&2; exit 1; }
# Older names of the same program go, so the folder holds one exe and no one runs a stale one by mistake.
if [[ "$built" != "$exe" ]]; then
  rm -f "$exe"
  cp "$built" "$exe"
fi
rm -f "$release/digup-shell.exe" "$release/DigUp Plus.exe" "$release/DigUp.exe"
rm -rf "$release/helper"
cp -r "$helper" "$release/helper"
cp shell/src-tauri/THIRD-PARTY-NOTICES.txt "$release/"
# Says this copy is portable: the shell keeps settings, logs, index and model in `data` here (options.rs) instead of
# %APPDATA% and %LOCALAPPDATA%. `--data <folder>` or DIGUP_DATA points them anywhere else.
printf 'DigUp Plus keeps its settings, logs, index and model in the data folder beside this file.\n' > "$release/portable.txt"
# The helper's hash decides whether a release also ships a shell-only zip (see docs/roadmap-plan.md): the same
# hash as the previous release means only DigUpPlus.exe moved, and updating is a small download. Cut releases check
# this line against the previous release's notes instead of trusting memory.
helper_sha=$(sha256sum "$release/helper/digup.exe" | cut -d" " -f1)
echo "HELPER SHA256: $helper_sha"
if [[ "${SHELL_ONLY:-0}" == "1" ]]; then
  small="$PWD/.build/DigUpPlus-$version-shell-only.zip"
  rm -f "$small"
  (cd "$release" && powershell -NoProfile -Command "Compress-Archive -Path 'DigUpPlus.exe','portable.txt','THIRD-PARTY-NOTICES.txt' -DestinationPath '$small' -Force")
  echo "SHELL-ONLY ZIP: $small"
fi
ls -la "$exe" "$release/helper" "$release/portable.txt" "$release/THIRD-PARTY-NOTICES.txt"
echo "DigUp Plus $version: run $exe"
