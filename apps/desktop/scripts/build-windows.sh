#!/bin/sh
# Build the Windows installer from WSL.
#
# Used by `pnpm --filter desktop build:windows`. `tauri build` only bundles for
# the OS it runs on, so this exports the committed HEAD (never uncommitted
# edits) to %LOCALAPPDATA%\pcai-build on the Windows side and runs
# build-windows.ps1 there through WSL interop. The installer it prints is built
# from exactly that commit.
#
# Exit code is the Windows build's own. 2 means WSL interop is unavailable and
# 3 that another run holds the build directory's lock.
set -eu

if ! command -v powershell.exe >/dev/null 2>&1 || ! command -v wslpath >/dev/null 2>&1; then
  echo "build:windows runs from WSL with Windows interop on (needs powershell.exe and wslpath)." >&2
  exit 2
fi

repo_root=$(git rev-parse --show-toplevel)
commit=$(git -C "$repo_root" rev-parse --short HEAD)
node_version=$(git -C "$repo_root" show HEAD:.nvmrc)
if [ -n "$(git -C "$repo_root" status --porcelain)" ]; then
  echo "warning: uncommitted changes are not included; building HEAD ($commit)." >&2
fi

local_app_data=$(powershell.exe -NoProfile -NonInteractive -Command '$env:LOCALAPPDATA' | tr -d '\r')
build_dir_win="$local_app_data\\pcai-build"
build_dir=$(wslpath -u "$build_dir_win")
mkdir -p "$build_dir"

# Two runs would share source.tar, src\ and out\, so take a lock first.
lock="$build_dir/lock"
if ! mkdir "$lock" 2>/dev/null; then
  echo "Another build:windows run holds $build_dir_win\\lock. If none is running, delete that folder." >&2
  exit 3
fi
trap 'rmdir "$lock"' EXIT
trap 'exit 130' INT TERM

# Windows creates symlinks only with Developer Mode or elevation, and the
# build needs none of the tracked ones (CLAUDE.md), so leave them out.
set --
for path in $(git -C "$repo_root" ls-tree -r HEAD | awk '$1 == "120000" { print $4 }'); do
  set -- "$@" ":(exclude)$path"
done
git -C "$repo_root" archive --format=tar -o "$build_dir/source.tar" HEAD -- . "$@"
git -C "$repo_root" show HEAD:apps/desktop/scripts/build-windows.ps1 >"$build_dir/build-windows.ps1"

powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
  -File "$build_dir_win\\build-windows.ps1" \
  -BuildDir "$build_dir_win" -Commit "$commit" -NodeVersion "$node_version"
