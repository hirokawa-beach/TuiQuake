#!/usr/bin/env bash
set -euo pipefail

executable=$(realpath "$1")
output=$(realpath -m "$2")
rust_notice=$(realpath "$3")
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=$("$executable" --version)
if [[ ! $version =~ ^TuiQuake\ ([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  printf 'Unsupported TuiQuake version: %s\n' "$version" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}
arch=$(dpkg --print-architecture)

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
cd -- "$work"
root=package
install -Dm755 "$executable" "$root/usr/bin/tuiquake"
install -Dm644 "$repo/LICENSE" "$root/usr/share/doc/tuiquake/copyright"
install -Dm644 "$repo/THIRD_PARTY_NOTICES.txt" "$root/usr/share/doc/tuiquake/THIRD_PARTY_NOTICES.txt"
install -Dm644 "$rust_notice" "$root/usr/share/doc/tuiquake/RUST-STDLIB-NOTICES.html"

mkdir -p "$root/DEBIAN" debian
cat > debian/control <<'EOF'
Source: tuiquake
Maintainer: hirokawa-beach
Section: utils
Priority: optional

Package: tuiquake
Architecture: any
Description: Japanese earthquake, early warning and tsunami information in the terminal
EOF

deps=$(dpkg-shlibdeps -O "$root/usr/bin/tuiquake")
if [[ $deps != shlibs:Depends=?* ]]; then
  printf 'Could not determine shared library dependencies: %s\n' "$deps" >&2
  exit 1
fi
cat > "$root/DEBIAN/control" <<EOF
Package: tuiquake
Version: $version
Architecture: $arch
Maintainer: hirokawa-beach
Section: utils
Priority: optional
Depends: ${deps#shlibs:Depends=}
Homepage: https://github.com/hirokawa-beach/TuiQuake
Description: Japanese earthquake, early warning and tsunami information in the terminal
EOF

dpkg-deb --root-owner-group --build "$root" "$output"
