#!/bin/sh
# Builds the voacapl engine and its itshfbc data tree from a pinned upstream release.
#
# Runs on Linux, macOS and Windows (MSYS2 UCRT64 shell).
# Needs: git, gfortran, make, autoconf, automake.
#
# Output (default .work/engine):
#   bin/voacapl[.exe]   the engine
#   itshfbc/            data tree the engine is pointed at
set -eu

VOACAPL_TAG="${VOACAPL_TAG:-v0.7.7}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="${WORK:-$ROOT/.work}"
SRC="$WORK/voacapl"
STAGE="$WORK/stage"
OUT="${OUT:-$WORK/engine}"

case "$(uname -s)" in
  MINGW*|MSYS*) EXE=.exe; ENGINE_LDFLAGS="-static" ;;
  *)            EXE=;     ENGINE_LDFLAGS="" ;;
esac

mkdir -p "$WORK"
if [ ! -d "$SRC/.git" ]; then
  git -c core.autocrlf=false clone --quiet --depth 1 --branch "$VOACAPL_TAG" \
    https://github.com/jawatson/voacapl "$SRC"
fi

cd "$SRC"

# Leave out the GPL-3 dst2csv/dst2ascii utilities. The engine does not use them.
sed -i.bak '/voacapl\/itshfbc\/bin\/dst\/Makefile/d' configure.ac
sed -i.bak \
  -e 's|\(voacapl/itshfbc/bin/anttyp99\) *\\$|\1|' \
  -e '/^[[:space:]]*voacapl\/itshfbc\/bin\/dst$/d' \
  Makefile.am

autoreconf -fi
# A fixed prefix plus DESTDIR keeps upstream's install hooks working under
# MSYS2, where an empty DESTDIR turns "/c/..." into the UNC path "//c/...".
./configure --prefix=/engine LDFLAGS="$ENGINE_LDFLAGS"
make
rm -rf "$STAGE"
make install DESTDIR="$STAGE"

rm -rf "$OUT"
mkdir -p "$OUT/bin"
cp "$STAGE/engine/bin/voacapl$EXE" "$OUT/bin/"
cp -R "$STAGE/engine/share/voacapl/itshfbc" "$OUT/itshfbc"

echo "Engine built: $OUT/bin/voacapl$EXE"
echo "Data tree:    $OUT/itshfbc"
