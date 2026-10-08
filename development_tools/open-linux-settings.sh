#!/bin/sh
set -eu
umask 077
data=${HERALD_DATA:-${XDG_DATA_HOME:-$HOME/.local/share}/herald}
settings=$data/settings.json
mkdir -p "$data"
if [ ! -f "$settings" ]; then
    (set -C; printf '{}\n' > "$settings")
fi
if command -v omarchy >/dev/null 2>&1; then
    exec omarchy launch editor "$settings"
fi
exec xdg-terminal-exec "${EDITOR:-vi}" "$settings"
