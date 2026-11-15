# 1. Rust core with thin TypeScript and Python bindings

Date: 2026-11-14. Status: accepted. Decided in [#7](https://github.com/jpleva91/harness/issues/7).

## Context

The wedge is a safe in-process harness for product teams ([#5](https://github.com/jpleva91/harness/issues/5)). In-process means the language decides who can import it. Product teams that build agents work in TypeScript and in Python.

## Decision

The harness is one Rust core. TypeScript and Python bindings both ship in v1, TypeScript first. The bindings are thin: the loop, permissions, sandbox, sessions and context live in the core, and the host language adds typed wrappers and the builder's callbacks. The Rust crate is internal in v1. Packages are prebuilt for Linux x64 and arm64 and macOS arm64. Windows is not a v1 platform.

## Alternatives rejected

- **TypeScript first.** Faster to build, but Python teams get no in-process harness and a later port means two implementations of the safety logic.
- **Python first.** Same problem in the other direction, with weaker typing for the safety model.
- **A Go or Rust library only.** Most product teams would have to consume it as a process, which is against the wedge.

## Consequences

- Every builder callback (custom tool, approval handler) crosses the FFI boundary. The loop prototype ([#10](https://github.com/jpleva91/harness/issues/10)) must prove this first.
- Releases need a native build per platform for npm and for PyPI.
- The core can change freely while the two public APIs settle.
