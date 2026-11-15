#!/usr/bin/env bash
# Drive the loop prototype the way a builder does: a host script imports the binding and runs sessions.
# Usage: .claude/skills/verify-loop-prototype/verify.sh launch | doctor | drive [node|py|all] | cleanup
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
proto=$repo/prototype/loop-ffi
node_lib=$proto/node/harness.node
py_lib=$proto/py/python/harness_proto/_native.so

launch() {
  set -e
  cd "$proto"
  PYO3_PYTHON=python3 cargo build --release
  cp target/release/libproto_node.so "$node_lib"
  cp target/release/lib_native.so "$py_lib"
  echo "READY: both bindings built"
}

doctor() {
  local bad=0
  say() { echo "$1  $2"; [ "$1" = OK ] || bad=1; }
  for tool in cargo node python3; do
    command -v $tool >/dev/null && say OK "$tool: $($tool --version 2>&1 | head -1)" || say MISSING "$tool"
  done
  for lib in "$node_lib" "$py_lib"; do
    if [ ! -f "$lib" ]; then say MISSING "${lib#$repo/} (run: verify.sh launch)"; continue; fi
    # A binding older than any Rust source is not the build under test.
    newer=$(find "$proto/core/src" "$proto/node/src" "$proto/py/src" "$proto"/*/Cargo.toml -newer "$lib" | head -1)
    [ -z "$newer" ] && say OK "${lib#$repo/} is current" || say STALE "${lib#$repo/} is older than ${newer#$repo/} (run: verify.sh launch)"
  done
  echo "revision: $(git -C "$repo" rev-parse --short HEAD)$(git -C "$repo" diff --quiet -- prototype || echo ' +uncommitted changes')"
  return $bad
}

# drive_one <name> <command...>: summary transcript, full event trace, exit code.
drive_one() {
  local name=$1; shift
  timeout 120 "$@" >"$out/$name.transcript.txt" 2>"$out/$name.stderr.txt"
  local code=$?
  echo "$code" >"$out/$name.exit"
  # The trace is a second run. It only counts as evidence if it agrees with the first.
  TRACE=1 timeout 120 "$@" >"$out/$name.trace.txt" 2>"$out/$name.trace.stderr.txt"
  local trace_code=$?
  verdicts() { grep -E '^(PASS|FAIL)  ' "$1" | sed -E 's/^((PASS|FAIL)  [^0-9{]*).*/\1/; s/ +$//'; }
  echo "--- $name: exit $code (124 means it hung and was killed)"
  cat "$out/$name.transcript.txt"
  [ -s "$out/$name.stderr.txt" ] && { echo "stderr:"; cat "$out/$name.stderr.txt"; }
  if [ $trace_code -ne $code ] || [ "$(verdicts "$out/$name.transcript.txt")" != "$(verdicts "$out/$name.trace.txt")" ]; then
    echo "TRACE MISMATCH: the trace run (exit $trace_code) disagrees with the first run; the trace is not evidence for it"
    return 1
  fi
  return $code
}

drive() {
  doctor >/dev/null || { doctor; echo "Doctor failed; not driving."; return 2; }
  out=$proto/evidence/$(date -u +%Y%m%dT%H%M%SZ)-$$
  mkdir -p "$out"
  doctor >"$out/doctor.txt"
  local which=${1:-all} bad=0
  case $which in node|all) drive_one node node "$proto/node/demo.mjs" || bad=1 ;; esac
  case $which in py|all) drive_one py python3 "$proto/py/demo.py" || bad=1 ;; esac
  echo "evidence: ${out#$repo/}"
  return $bad
}

cleanup() {
  # Drives are short-lived processes under a timeout; nothing stays running. Evidence is kept.
  rm -f "$node_lib" "$py_lib"
  echo "removed built bindings; evidence kept in prototype/loop-ffi/evidence/"
}

case ${1:-} in
  launch) launch ;;
  doctor) doctor ;;
  drive) drive "${2:-all}" ;;
  cleanup) cleanup ;;
  *) sed -n '2,3p' "$0"; exit 2 ;;
esac
