# Compares two normalised VOACAP output files field by field.
#
#   awk -f compare-out.awk expected actual
#
# Text fields must match exactly. A numeric field passes if it is identical,
# within one unit of its last printed digit, or within 1% of the expected
# value. The first two cover rounding noise between compilers; the third
# covers iterated quantities such as virtual height. All three are counted
# separately so drift stays visible. Exit status is 1 on any other difference.

function isnum(s) { return s ~ /^[-+]?([0-9]+\.?[0-9]*|\.[0-9]+)$/ }
function decimals(s,   i) { i = index(s, "."); return i ? length(s) - i : 0 }
function abs(x) { return x < 0 ? -x : x }
function mismatch(n, what, e, a) {
  bad++
  if (bad <= 10) printf "  line %d: %s\n    expected: %s\n    actual:   %s\n", n, what, e, a
}

BEGIN {
  expfile = ARGV[1]; actfile = ARGV[2]
  while ((getline e < expfile) > 0) {
    n++
    if ((getline a < actfile) <= 0) { mismatch(n, "actual output ends early", e, ""); break }
    ne = split(e, E, " "); na = split(a, A, " ")
    if (ne != na) { mismatch(n, "different number of fields", e, a); continue }
    for (i = 1; i <= ne; i++) {
      if (E[i] == A[i]) { if (isnum(E[i])) exact++; continue }
      if (isnum(E[i]) && isnum(A[i])) {
        d = decimals(E[i]); if (decimals(A[i]) > d) d = decimals(A[i])
        diff = abs(E[i] - A[i])
        if (diff <= 1.001 * 10 ^ (-d)) { near++; continue }
        if (diff <= 0.01 * abs(E[i])) { percent++; continue }
      }
      mismatch(n, "field " i " differs", e, a)
    }
  }
  if ((getline a < actfile) > 0) mismatch(n + 1, "actual output has extra lines", "", a)
  printf "%d fields identical, %d within one last-digit unit, %d within 1%%, %d mismatches\n", exact, near, percent, bad
  exit bad ? 1 : 0
}
