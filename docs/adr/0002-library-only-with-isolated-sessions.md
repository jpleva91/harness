# 2. Library only in v1, with isolated sessions

Date: 2026-11-14. Status: accepted. Decided in [#8](https://github.com/jpleva91/harness/issues/8).

## Context

The wedge is a safe in-process harness ([#5](https://github.com/jpleva91/harness/issues/5)). Existing harnesses that embed as a subprocess or server collect most of their embedding bugs at the process boundary ([#2](https://github.com/jpleva91/harness/issues/2)). Product teams run many tenants in one process.

## Decision

v1 is a library only. The builder holds sessions. Many sessions run in one process, and each is given its own config, credentials, workspace and sandbox. Sessions share nothing. The TypeScript and Python APIs have one shape and are async only. An example command-line tool ships without a support promise.

## Alternatives rejected

- **A headless process or protocol mode in v1.** It widens v1 and brings back the process-boundary bugs the wedge avoids.
- **A shared harness instance that sessions inherit from.** Less to pass in, but shared state is how tenants leak into each other.
- **One session per process.** It gives up the in-process wedge for multi-tenant products.

## Consequences

- A process or protocol mode after v1 must be a wrapper over the library.
- Builders in other languages have no supported path in v1.
- Everything a session needs is passed in explicitly, which makes the API more verbose.
