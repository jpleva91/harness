#!/usr/bin/env bash
# Check the loop's cancel and approval rules: the TLA+ model, its mutants, and the replay tests.
# Usage: .claude/skills/formal-verify-loop/check.sh
# Needs Java 11 or later (set JAVA, or have `java` on PATH). Fetches the pinned TLC jar on first run.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
model=$here/model

jar=${TLA2TOOLS:-$HOME/.cache/tlaplus/tla2tools.jar}
sha=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
if [ ! -f "$jar" ]; then
  mkdir -p "$(dirname "$jar")"
  curl -sSLo "$jar" https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
fi
echo "$sha  $jar" | sha256sum -c --quiet -
java=${JAVA:-$([ -x "$HOME/.cache/tlaplus/jre/bin/java" ] && echo "$HOME/.cache/tlaplus/jre/bin/java" || echo java)}
major=$("$java" -version 2>&1 | sed -n '1s/.*"\([0-9]*\)\..*/\1/p')
[ "${major:-0}" -ge 11 ] || { echo "TLC needs Java 11 or later; set JAVA to one. Found: $("$java" -version 2>&1 | head -1)" >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0

# tlc <dir>: prints the name of the rule TLC says is violated, or nothing when every rule holds.
tlc() {
  (cd "$1" && "$java" -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 -metadir "$1/states" -config Loop.cfg Loop.tla >tlc.out 2>&1) || true
  grep -q "Model checking completed. No error has been found." "$1/tlc.out" && return 0
  sed -n 's/^Error: Invariant \(.*\) is violated\.$/\1/p' "$1/tlc.out" | head -1 | grep . || { echo "TLC_ERROR"; }
}

echo "== model: every rule must hold"
mkdir "$work/base" && cp "$model"/Loop.* "$work/base/"
got=$(tlc "$work/base")
if [ -z "$got" ]; then
  echo "PASS  $(grep -o '[0-9]* distinct states found' "$work/base/tlc.out" | tail -1)"
else
  echo "FAIL  $got"; fail=1
  sed -n '/^Error:/,/^[0-9]* states generated/p' "$work/base/tlc.out"
fi

echo "== mutants: each rule must fail on its broken variant"
# mutant <name> <rule that must break> <file> <sed script>
mutant() {
  mkdir "$work/$1" && cp "$model"/Loop.* "$work/$1/"
  before=$(cksum <"$work/$1/$3")
  sed -i "$4" "$work/$1/$3"
  [ "$before" != "$(cksum <"$work/$1/$3")" ] || { echo "FAIL  $1: the mutation matched nothing; update check.sh to the model"; fail=1; return; }
  got=$(tlc "$work/$1")
  if [ "$got" = "$2" ]; then echo "PASS  $1 breaks $2"; else echo "FAIL  $1: wanted $2 to break, got '${got:-no violation}'"; fail=1; fi
}
mutant unbiased-select NoHostCallStartsAfterCancel Loop.cfg 's/CancelFirst = TRUE/CancelFirst = FALSE/'
mutant error-allows ToolRunsOnlyWhenAllowed Loop.tla "s/allowed' = (approval = \"allow\")/allowed' = (approval # \"deny\")/"
mutant no-abort NoOrphanHostCall Loop.tla "s/tool' = (IF tool = \"running\" THEN \"aborted\" ELSE tool)/tool' = tool/"

echo "== code: replay tests against the real loop"
if (cd "$repo/prototype/loop-ffi" && cargo test -q -p proto-core --test loop_rules 2>&1 | tee "$work/cargo.out" | grep -E "^test .*FAILED|cancelled runs|test result"); then
  grep -q "test result: ok" "$work/cargo.out" || fail=1
else
  cat "$work/cargo.out"; fail=1
fi

[ $fail -eq 0 ] && echo "ALL CHECKS PASS" || { echo "CHECKS FAILED"; exit 1; }
