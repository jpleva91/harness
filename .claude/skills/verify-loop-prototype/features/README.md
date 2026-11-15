# Loop prototype verification map

The maintained source for verifying what a builder can observe from the loop prototype. Read this index, then use the matching feature file as the recipe.

## Baseline preconditions

- `verify.sh launch` has printed `READY: both bindings built`.
- `verify.sh doctor` prints only `OK` lines.
- Every recipe starts from a fresh process. Sessions share nothing, so there is no state to reset.

## Driving conventions

- Drive through the wrappers: `startSession` in `prototype/loop-ffi/node/harness.mjs`, `start_session` in `prototype/loop-ffi/py/python/harness_proto`.
- Every behavior is checked in both bindings. A feature proven in Node only is not proven.
- Scenario names below are the exact text of the `PASS`/`FAIL` lines. The Python demo says `raises` where Node says `throws`.
- Commands are literal and run from the repo root. `verify.sh` means `.claude/skills/verify-loop-prototype/verify.sh`.

## Proof and skip reporting

- Proof is the scenario line from `<binding>.transcript.txt` plus the events above it in `<binding>.trace.txt`.
- Record the evidence directory and the revision from `doctor.txt` with every claim.
- If a binding cannot be built, report the build error and mark every feature unverified for that binding. Do not report the other binding's result in its place.

## Feature entry contract

Each feature file has an H1, one paragraph on what the builder sees, then four H2 sections in this order: `Sub-features`, `How to get to it (user POV)`, `Driving it with verify.sh`, `Gotchas`.

## Features

- [Host callbacks](./host-callbacks.md): custom tools and the approval handler, sync and async, and what happens when they fail.
- [Cancellation and timeouts](./cancellation.md): cancel during a tool or an approval, tool timeouts, and what the host callback sees.
- [Event stream](./event-stream.md): event order, steering, retries, backpressure, many sessions, and an abandoned session.
