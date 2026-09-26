#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo"
cargo build --locked --profile release-lto --bin anastasia
data_root=${ANASTASIA_CLI_HOME:-"$HOME/.anastasia-cli"}
version=$(git rev-parse --short HEAD)-$(date +%Y%m%d%H%M%S)
destination="$data_root/builds/versions/$version"
mkdir -p "$destination" "$HOME/.local/bin"
chmod 700 "$data_root" "$data_root/builds" "$data_root/builds/versions" "$destination"
cp target/release-lto/anastasia "$destination/anastasia"
chmod 755 "$destination/anastasia"
# `anastasia` is the command; `ana` is the short alias for the same binary.
for launcher in "$HOME/.local/bin/anastasia" "$HOME/.local/bin/ana"; do
    if [ -e "$launcher" ] || [ -L "$launcher" ]; then
        mv "$launcher" "$launcher.backup-$version"
    fi
    ln -s "$destination/anastasia" "$launcher"
done
"$HOME/.local/bin/anastasia" --version
printf 'Installed %s and %s\n' "$HOME/.local/bin/anastasia" "$HOME/.local/bin/ana"
