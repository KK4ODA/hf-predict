#!/bin/sh
# Runs every deck in tests/engine/cases through the built engine.
#
# A deck with an expected output of the same name (produced by Windows VOACAP)
# is compared against it with compare-out.awk. A deck without one is only run;
# CI compares those outputs between operating systems.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ENGINE="${OUT:-$ROOT/.work/engine}"
RESULTS="$ROOT/.work/reference-out"
CASES="$ROOT/tests/engine/cases"
COMPARE="$ROOT/tests/engine/compare-out.awk"

# Decks run in a scratch run directory so the built engine tree stays clean.
RUN="$RESULTS/run"

case "$(uname -s)" in
  MINGW*|MSYS*) EXE=.exe; ITSHFBC="$(cygpath -w "$ENGINE/itshfbc")"; RUN_ARG="$(cygpath -w "$RUN")" ;;
  *)            EXE=;     ITSHFBC="$ENGINE/itshfbc";                 RUN_ARG="$RUN" ;;
esac

# Windows VOACAP prints antenna paths in upper case with backslashes, and
# voacapl adds an "L" to the version banner.
normalise() {
  tr -d '\r' < "$1" | tr 'A-Z\\' 'a-z/' \
    | sed -e 's/voacap l /voacap /' -e 's/[[:space:]]*$//'
}

rm -rf "$RESULTS"
mkdir -p "$RUN"
failed=0

for deck in "$CASES"/*.dat; do
  name="$(basename "$deck" .dat)"
  cp "$deck" "$RUN/$name.dat"

  if ! "$ENGINE/bin/voacapl$EXE" -s "--run-dir=$RUN_ARG" "$ITSHFBC" "$name.dat" "$name.out" \
       > "$RESULTS/$name.log" 2>&1; then
    echo "FAIL $name: engine exited with an error (see $RESULTS/$name.log)"
    failed=1
    continue
  fi
  if [ ! -s "$RUN/$name.out" ]; then
    echo "FAIL $name: engine produced no output (see $RESULTS/$name.log)"
    failed=1
    continue
  fi

  cp "$RUN/$name.out" "$RESULTS/$name.out"
  normalise "$RESULTS/$name.out" > "$RESULTS/$name.actual.norm"

  if [ ! -f "$CASES/$name.out" ]; then
    echo "RAN  $name: no expected output; $(wc -l < "$RESULTS/$name.out" | tr -d ' ') lines"
    continue
  fi

  normalise "$CASES/$name.out" > "$RESULTS/$name.expected.norm"
  if summary="$(awk -f "$COMPARE" "$RESULTS/$name.expected.norm" "$RESULTS/$name.actual.norm")"; then
    echo "PASS $name: $summary"
  else
    echo "FAIL $name:"
    echo "$summary"
    failed=1
  fi
done

exit "$failed"
