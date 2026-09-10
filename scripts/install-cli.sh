#!/bin/sh

set -eu

binary_path=${1:-./qfence}
install_dir=${QFENCE_INSTALL_DIR:-${XDG_BIN_HOME:-$HOME/.local/bin}}

if [ ! -f "$binary_path" ]; then
  echo "qfence installer: binary not found: $binary_path" >&2
  exit 1
fi

mkdir -p "$install_dir"
cp "$binary_path" "$install_dir/qfence"
cp "$binary_path" "$install_dir/quotafence"
chmod 755 "$install_dir/qfence" "$install_dir/quotafence"

# A copied linker-signed debug binary can pass `codesign --verify` but still be
# killed by dyld with "Code Signature Invalid" on recent macOS releases. Re-sign
# only ad-hoc development builds after copying. Official release signatures are
# intentionally left untouched.
if [ "$(uname -s)" = "Darwin" ] && command -v codesign >/dev/null 2>&1; then
  if codesign -dv "$binary_path" 2>&1 | grep -q 'Signature=adhoc'; then
    codesign --force --sign - --timestamp=none "$install_dir/qfence"
    codesign --force --sign - --timestamp=none "$install_dir/quotafence"
  fi
fi

case ":${PATH:-}:" in
  *":$install_dir:"*)
    echo "Installed qfence and quotafence in $install_dir"
    ;;
  *)
    echo "Installed qfence and quotafence in $install_dir"
    echo "Add this directory to PATH, then open a new terminal:"
    echo "  export PATH=\"$install_dir:\$PATH\""
    ;;
esac
