#!/bin/bash
# Builds DigUp's Windows installer: the Tauri shell (shell/) with DigUp's helper, llama.cpp (with the CUDA runtime it
# needs), PDFium, the Swift and C++ runtimes and ffmpeg in its helper folder, as an NSIS installer that installs for the
# current user (no admin prompt).
# Run it in Git for Windows' bash inside a Visual Studio developer environment, after scripts/fetch-vendor.sh,
# scripts/build-llama.sh and scripts/build-ffmpeg.sh (that one in MSYS2's MINGW64 shell). Needs Swift, Rust, Python and
# Node (for Tauri's CLI):
#   scripts/package-windows.sh      → shell/src-tauri/target/release/bundle/nsis/
set -euo pipefail
cd "$(dirname "$0")/.."
[[ -x Vendor/ffmpeg/bin/ffmpeg.exe ]] || { echo "no Vendor/ffmpeg/bin/ffmpeg.exe: run scripts/build-ffmpeg.sh in MSYS2" >&2; exit 1; }
TAURI_CLI=2.12.2

swift build -c release --product digup
# SwiftPM 6.4 can't make the .build/release link on Windows (I/O error 512), so ask it where the exe is.
bin=$(swift build -c release --show-bin-path)

# What the installer puts beside DigUp.exe, in helper\ ("DigUp.exe" and "digup.exe" can't share a folder): the helper
# finds the DLLs, llama.cpp's backends and ffmpeg in its own folder. The C++ runtime goes with them, so nothing needs
# installing first; the shell itself links its C runtime statically (.cargo/config.toml).
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
cuda_root=$(cygpath "${CUDA_PATH:-C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA}")
cuda=$(ls -d "$cuda_root"/bin/x64 2>/dev/null || ls -d "$cuda_root"/*/bin/x64 2>/dev/null | sort -V | tail -1 || true)
[[ -n "$cuda" ]] || { echo "no CUDA runtime in $cuda_root/bin/x64: set CUDA_PATH (the helper would run on the processor)" >&2; exit 1; }
cp "$cuda"/cublas64_*.dll "$cuda"/cublasLt64_*.dll "$cuda"/cudart64_*.dll "$helper/"
cp "$swift_runtime"/*.dll "$helper/"
cp "$crt"/*.dll "$helper/"   # over the Swift runtime's copies of the same: the newer C++ runtime
cp Vendor/ffmpeg/bin/ffmpeg.exe Vendor/ffmpeg/bin/ffprobe.exe "$helper/"   # the small static build, no DLLs

# No update signing: DigUp Plus ships by hand (scripts/package-portable.sh is what most builds go out as), and the
# in-app updater this fork had from upstream is gone rather than pointed at a feed of its own.
# The license of everything the installer carries, installed beside the app. (python3 can be the Store's stub.)
python=python3
python3 -c '' 2> /dev/null || python=python
CUDA_RUNTIME_DIR="$cuda" "$python" scripts/notices.py windows shell/src-tauri/THIRD-PARTY-NOTICES.txt   # its license too
# Installers of an earlier build (another version) would be taken for this one's.
rm -rf shell/src-tauri/target/release/bundle/nsis
# The helper folder goes in as the installer's resources here only: Tauri checks resources on every build.
resources='{"helper/":"helper/","THIRD-PARTY-NOTICES.txt":"THIRD-PARTY-NOTICES.txt"}'
(cd shell/src-tauri && npx --yes "@tauri-apps/cli@$TAURI_CLI" build --bundles nsis \
  --config "{\"bundle\":{\"resources\":$resources}}")
ls -la shell/src-tauri/target/release/bundle/nsis/
