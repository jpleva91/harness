# PROTOTYPE: loop shape and FFI callbacks (throwaway)

Answers [#10](https://github.com/jpleva91/harness/issues/10). Not production code. Lives only on the `prototype/loop-ffi` branch.

Question 1 (from ADR 1): can the Rust loop call host-language callbacks (a custom tool and an approval handler) across the FFI boundary in Node and CPython, including async callbacks and cancellation?

Question 2: what loop unit and event stream should a builder see?

- `core/`: the loop, driven by a scripted fake model. No provider calls.
- `node/`: napi-rs binding plus `harness.mjs` (thin wrapper) and `demo.mjs` (scenarios).
- `py/`: PyO3 binding plus `harness_proto/__init__.py` (thin wrapper) and `demo.py` (same scenarios).

Run: `./run.sh` builds both bindings and runs both demos.
