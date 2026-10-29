#!/bin/sh
# Cross build using the verified toolchain downloaded into .tools/. Native Windows
# users simply run `cargo build --release --locked` instead.
set -eu
RELAY_PROJECT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
RELAY_MINGW_ROOT=${RELAY_MINGW_ROOT:-$RELAY_PROJECT/.tools/llvm-mingw-20261020-ucrt-ubuntu-22.04-x86_64}
export PATH="$RELAY_MINGW_ROOT/bin:$PATH"
export RUSTC="$RELAY_PROJECT/tools/rustc-cross.sh"
export CC_x86_64_pc_windows_gnullvm="$RELAY_MINGW_ROOT/bin/x86_64-w64-mingw32-clang"
export AR_x86_64_pc_windows_gnullvm="$RELAY_MINGW_ROOT/bin/llvm-ar"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER="$RELAY_MINGW_ROOT/bin/x86_64-w64-mingw32-clang"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS='-C target-feature=+crt-static'
cd "$RELAY_PROJECT"
cargo build --release --locked --offline --target x86_64-pc-windows-gnullvm --target-dir target/windows --jobs 4
