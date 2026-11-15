---
name: verify-loop-prototype
description: "Drive the harness loop prototype (prototype/loop-ffi) the way an agent builder does, from a Node script and a Python script that import the bindings, and capture proof. Use to prove a change to the loop, the Node binding or the Python binding still works: host callbacks, approvals, cancellation, the event stream."
---

# Verify the loop prototype

The prototype is a library, not an app. Its user is an agent builder whose own script imports a binding, starts sessions, hands in a tool and an approval handler, and reads events. So the user path is a host script: `prototype/loop-ffi/node/demo.mjs` (through `node/harness.mjs`) and `prototype/loop-ffi/py/demo.py` (through `py/python/harness_proto`). Both run the same scenarios against a scripted model (14 in Node, 15 in Python, which adds a dropped-session scenario), then exit with one session abandoned on purpose.

There is no model provider, network, port, database or auth. Two runs can go side by side; only `cargo build` takes a lock.

All commands run from the repo root through one helper:

```sh
.claude/skills/verify-loop-prototype/verify.sh launch | doctor | drive [node|py|all] | cleanup
```

## Launch

```sh
.claude/skills/verify-loop-prototype/verify.sh launch
```

Builds the Rust core and both bindings in release mode and copies them to `prototype/loop-ffi/node/harness.node` and `prototype/loop-ffi/py/python/harness_proto/_native.so`. Ready when it prints `READY: both bindings built`. The first build takes about a minute; later ones a few seconds. Needs `cargo`, Node 20 or later, and Python 3 with its headers (`python3-dev`). Nothing stays running: each drive is a short process.

## Doctor

```sh
.claude/skills/verify-loop-prototype/verify.sh doctor
```

Read-only. Prints `OK`, `MISSING` or `STALE` per tool and per binding, and the git revision. `STALE` means a Rust source is newer than the built binding, so a drive would test the old build: run `launch`. `drive` refuses to run when doctor fails.

## Drive

```sh
.claude/skills/verify-loop-prototype/verify.sh drive        # both bindings
.claude/skills/verify-loop-prototype/verify.sh drive node   # or: py
```

Each binding prints one `PASS` or `FAIL` line per scenario and a last line `14/14 passed (...)` for Node or `15/15 passed (...)` for Python. Exit code 0 means every scenario passed and the process exited by itself. Exit 124 means the process hung and was killed after 120 seconds, which is itself a finding: the last scenario abandons a session, and the process must still exit.

To drive something the scenarios do not cover, write a host script next to the demo that imports `startSession` (Node) or `start_session` (Python) and run it with `node` or `python3`. Use the wrapper, not the native module: the wrapper is the surface a builder would get. Which scenario proves which feature is in [`features/`](features/README.md).

## Evidence

Each drive writes `prototype/loop-ffi/evidence/<UTC time>-<pid>/` (ignored by git) and prints its path:

- `doctor.txt`: tools, build freshness and revision.
- `<binding>.transcript.txt`, `<binding>.stderr.txt`, `<binding>.exit`: the scenario lines, errors and exit code.
- `<binding>.trace.txt`, `<binding>.trace.stderr.txt`: a second run with every event printed as JSON above the scenario line it belongs to. `drive` fails with `TRACE MISMATCH` if this run's exit code or verdicts differ from the first run's.

Proof standards for this prototype:

- A `PASS` line alone is not proof of an ordering claim. Quote the events from the trace.
- A claim about the host side (the tool saw the abort, the tool never ran) must come from a flag the host callback set, which each scenario checks, not from the event stream alone.
- Timing numbers (`ms after cancel`, `us per tool call`, `event-loop lag`) vary by machine and build. Report them with the build mode; never gate on them beyond the scenario's own threshold.
- The model is scripted. Nothing here proves behavior against a real provider.

## Cleanup

```sh
.claude/skills/verify-loop-prototype/verify.sh cleanup
```

Removes the two built bindings. It never removes `evidence/`, and it kills nothing: the helper starts every drive under `timeout`, so no process outlives it. `target/` is left for the next build; remove it with `cargo clean` in `prototype/loop-ffi` if disk matters.

## Related

`/formal-verify-loop` checks the cancel and approval rules over every ordering. Run both after a change to `prototype/loop-ffi/core/src/lib.rs`.
