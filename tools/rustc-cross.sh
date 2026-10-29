#!/bin/sh
set -eu
RELAY_PROJECT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec "$RELAY_PROJECT/.tools/rustc-1.98.0-x86_64-unknown-linux-gnu/rustc/bin/rustc" --sysroot "$RELAY_PROJECT/.tools/rust-sysroot" "$@"
