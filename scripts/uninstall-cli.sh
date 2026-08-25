#!/bin/sh

set -eu

install_dir=${QFENCE_INSTALL_DIR:-${XDG_BIN_HOME:-$HOME/.local/bin}}

rm -f "$install_dir/qfence" "$install_dir/quotafence"
echo "Removed qfence and quotafence from $install_dir"

