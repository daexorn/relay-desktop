#!/bin/sh
set -eu
RELAY_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install -Dm0755 "$RELAY_DIR/relay" "$HOME/.local/bin/relay.pending"
mv -f "$HOME/.local/bin/relay.pending" "$HOME/.local/bin/relay"
install -Dm0644 "$RELAY_DIR/relay.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/relay.svg"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/relay.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Relay
Comment=Claude Code on your servers
Exec="$HOME/.local/bin/relay"
Icon=relay
Terminal=false
Categories=Development;Network;
StartupWMClass=Relay
EOF
printf '%s\n' 'Relay установлен в меню приложений.'
