#!/usr/bin/env bash
# PROTOTYPE. One command: builds the probe and prints every result. Needs Linux with Landlock.
set -u
cd "$(dirname "$0")"
cargo build --release -q || exit 1
SBX=$PWD/target/release/sbx
D=$(mktemp -d /var/tmp/sbx-probe.XXXXXX)   # PROTOTYPE scratch, wiped at exit
trap 'kill $VICTIM_PID $LISTENER 2>/dev/null; rm -rf "$D"' EXIT
mkdir -p "$D"/{ws,tmp,out,ro}
echo secret > "$D/out/secret"; chmod 600 "$D/out/secret"; echo data > "$D/ro/file"
export OUT=$D/out RO=$D/ro WS=$D/ws SOCK=$D/out/sock TMPDIR=$D/tmp HOME=$D/ws GIT_CONFIG_NOSYSTEM=1
sleep 600 & VICTIM_PID=$!; export VICTIM_PID
python3 - <<PY &
import socket, time
a = socket.socket(socket.AF_UNIX); a.bind("$SOCK"); a.listen()
b = socket.socket(socket.AF_UNIX); b.bind("\0sbx-probe"); b.listen()
time.sleep(600)
PY
LISTENER=$!; sleep 0.5
SYS=(--ro /usr --ro /bin --ro /lib --ro /lib64 --ro /etc --ro /proc --ro "$PWD" --rw /dev/null --ro /dev/urandom)
BASE=("${SYS[@]}" --ro "$D/ro" --rw "$D/ws" --rw "$D/tmp")

echo "kernel $(uname -r), Landlock rights limited to ABI 3"
for sc in "" net,meta,signal; do
  echo; echo "== attacks, seccomp: ${sc:-none} =="
  "$SBX" "${BASE[@]}" ${sc:+--seccomp $sc} -- python3 probe.py
done

echo; echo "== attacks, Landlock ABI 6 (kernel 6.12), seccomp: net =="
"$SBX" --abi6 "${BASE[@]}" --seccomp net -- python3 probe.py | grep -E 'signal|Unix|ALLOWED'
( cd "$D/ws" && "$SBX" --abi6 "${BASE[@]}" --seccomp net -- sh -c 'sleep 3 & kill $!' ) && echo "  ok       shell kills its own background job" || echo "  BROKEN   shell kills its own background job"

echo; echo "== ordinary tools in the workspace, per seccomp group =="
cat > "$D/ws/hello.c" <<<'int main(){return 0;}'
( cd "$D/ws" && mkdir t && echo x > t/a && chmod 755 t/a && tar cf t.tar t && rm -r t )
tools=(
  "touch f"
  "chmod +x hello.c"
  "cp -p hello.c copy.c"
  "install -m 755 hello.c inst"
  "tar xf t.tar"
  "gcc hello.c -o hello"
  "git init -q . && git add hello.c && git -c user.name=p -c user.email=p@p commit -qm x"
  "python3 -c 'import tempfile; tempfile.mkdtemp()'"
  "sh -c 'sleep 3 & kill \$!'"
  "timeout 0.2 sleep 3; test \$? = 124"
  "python3 -c 'import asyncio; asyncio.run(asyncio.sleep(0))'"
  "python3 -c 'import multiprocessing as m; m.Queue()'"
)
for sc in net meta signal; do
  echo "-- seccomp: $sc"
  for t in "${tools[@]}"; do
    if ( cd "$D/ws" && timeout 10 "$SBX" "${BASE[@]}" --seccomp $sc -- sh -c "$t" ) >/dev/null 2>"$D/err"; then r=ok; else r="BROKEN  $(head -c 90 "$D/err" | tr '\n' ' ')"; fi
    [ "$r" = ok ] || echo "  $t -> $r"
    rm -rf "$D/ws"/{f,copy.c,inst,t,hello,.git}
  done
done

echo; echo "== .git read-only inside a read-write workspace =="
( cd "$D/ws" && git init -q . && git add hello.c && git -c user.name=p -c user.email=p@p commit -qm x && mkdir src && echo a > src/a )
echo "-- A: whole workspace read-write (no protection)"
PROT='echo x > .git/hooks/pre-commit && echo "  ALLOWED  write .git/hooks" || echo "  blocked  write .git/hooks"
echo x >> src/a && echo "  ok       edit an existing file" || echo "  BROKEN   edit an existing file"
echo x > src/new && echo "  ok       create a file in an existing directory" || echo "  BROKEN   create a file in an existing directory"
echo x > top && echo "  ok       create a top-level file" || echo "  BROKEN   create a top-level file"
mkdir newdir && echo "  ok       create a top-level directory" || echo "  BROKEN   create a top-level directory"
git status >/dev/null && echo "  ok       git status" || echo "  BROKEN   git status"'
( cd "$D/ws" && "$SBX" "${BASE[@]}" -- sh -c "$PROT" 2>/dev/null ); rm -rf "$D/ws"/{top,newdir,src/new,.git/hooks/pre-commit}
echo "-- B: workspace read-only, each top-level entry except .git read-write"
ENTRIES=(); for e in "$D/ws"/* "$D/ws"/.[!.]*; do [ -e "$e" ] && [ "$(basename "$e")" != .git ] && ENTRIES+=(--rw "$e"); done
( cd "$D/ws" && "$SBX" "${SYS[@]}" --ro "$D/ws" "${ENTRIES[@]}" --rw "$D/tmp" -- sh -c "$PROT" 2>/dev/null )

echo "-- C: git directory outside the workspace, read-only; workspace read-write"
rm -rf "$D/ws"/{top,newdir,src/new}; mv "$D/ws/.git" "$D/gitdir"; echo "gitdir: $D/gitdir" > "$D/ws/.git"
( cd "$D/ws" && "$SBX" "${BASE[@]}" --ro "$D/gitdir" -- sh -c "echo x > $D/gitdir/hooks/pre-commit"' && echo "  ALLOWED  write git hooks" || echo "  blocked  write git hooks"
git status >/dev/null && echo "  ok       git status" || echo "  BROKEN   git status"
echo x > top && echo "  ok       create a top-level file" || echo "  BROKEN   create a top-level file"
echo "gitdir: $PWD/evil" > .git && echo "  ALLOWED  repoint the .git file at a directory the agent controls" || echo "  blocked  repoint the .git file"' 2>/dev/null )

echo; echo "== spawn cost, 300 runs of /bin/true =="
"$SBX" "${BASE[@]}" --seccomp net,meta,signal --bench 300 -- /bin/true
