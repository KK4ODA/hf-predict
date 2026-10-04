#!/bin/sh
# Compares each platform's engine outputs against the Linux build.
#
#   sh compare-platforms.sh <dir holding one engine-<target> folder per platform>
#
# SAME  = identical text, CLOSE = within the tolerances of compare-out.awk,
# DIFF  = outside them (fails).
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPARE="$ROOT/tests/engine/compare-out.awk"
ART="$1"
BASE="$ART/engine-linux-x64/reference-out"
failed=0

for other in "$ART"/engine-*; do
  target="$(basename "$other")"
  [ "$target" = engine-linux-x64 ] && continue
  for base in "$BASE"/*.actual.norm; do
    name="$(basename "$base" .actual.norm)"
    theirs="$other/reference-out/$name.actual.norm"
    if [ ! -f "$theirs" ]; then
      echo "DIFF  $target $name: output missing"
      failed=1
    elif cmp -s "$base" "$theirs"; then
      echo "SAME  $target $name"
    elif summary="$(awk -f "$COMPARE" "$base" "$theirs")"; then
      echo "CLOSE $target $name: $summary"
    else
      echo "DIFF  $target $name:"
      echo "$summary"
      failed=1
    fi
  done
done

exit "$failed"
