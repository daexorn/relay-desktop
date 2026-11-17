#!/bin/sh
set -eu
RELAY_TUI_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install -Dm0755 "$RELAY_TUI_DIR/relay-tui" "$HOME/.local/bin/relay-tui.pending"
mv -f "$HOME/.local/bin/relay-tui.pending" "$HOME/.local/bin/relay-tui"
printf '%s\n' 'Relay TUI установлен. Запуск: relay-tui'
