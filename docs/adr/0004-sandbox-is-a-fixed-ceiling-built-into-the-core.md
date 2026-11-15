# 4. The sandbox is a fixed ceiling built into the core

Date: 2026-11-15. Status: accepted. Decided in [#11](https://github.com/jpleva91/harness/issues/11).

## Context

The wedge is the safe in-process harness ([#5](https://github.com/jpleva91/harness/issues/5)): deny unless allowed, sandbox on, network off. The isolation research ([#4](https://github.com/jpleva91/harness/issues/4)) found that existing harnesses let an approval lift a call out of the sandbox, run their file tools in-process behind path checks, and fall back to no sandbox when a dependency is missing. The advisories reviewed in #4 were all policy or path bugs, not kernel escapes. On Ubuntu 24.04 AppArmor blocks bubblewrap by default.

## Decision

Permissions decide whether a tool call starts. The sandbox bounds what a started call can touch. The two do not trade against each other.

- The sandbox is set in the session's config and is always on for the built-in shell and file tools. No permission rule and no approval widens it. To widen it, the builder changes the session's config.
- The built-in file tools run under the same sandbox as the shell tool, not in-process behind path checks.
- The sandbox is built into the core with no external binary: Landlock plus seccomp on Linux, Seatbelt on macOS.
- The Linux floor is Landlock ABI 3, kernel 6.2. Below ABI 3 Landlock cannot stop a process truncating a file it may only read, so a protected path would not hold.
- Landlock does not control socket creation, so seccomp cuts the network.
- If the sandbox cannot be set up, creating the session fails. There is no unsandboxed fallback.
- Default reach: read and write in the workspace and a private temp directory, read-only on the system paths the builder lists, nothing else. `.git` and the harness config inside the workspace are read-only by default.
- Network is off by default. One switch on the session turns it fully on.
- A call the sandbox refuses is a tool failure, which goes back to the model.
- Builder tools run in the builder's process, outside the sandbox, and are governed by permission rules only.

## Alternatives rejected

- **An approval lifts a call out of the sandbox.** Codex does this. A prompt-injected agent then needs one mistaken approval.
- **A sandbox the builder can switch off.** Convenient inside a container, but it makes running unsandboxed possible by accident.
- **bubblewrap first.** Stronger (mount and PID namespaces), but it is an external or bundled binary and needs an AppArmor profile on Ubuntu 24.04.
- **A bubblewrap fallback below kernel 6.2.** Widest reach, but two Linux sandboxes to build and test in v1.
- **Landlock ABI 1 with seccomp closing the truncate gap.** Reaches kernel 5.13; whether seccomp closes the gap fully is unverified.
- **File tools in-process.** Faster, but a path or symlink bug then reads the builder's own files.
- **A domain-allowlist proxy in v1.** It fits the model later without changing it, and it is a sizeable component.

## Consequences

- Hosts the harness refuses: RHEL 8, RHEL 9 before 9.6, Ubuntu 22.04 on its original 5.15 kernel, and 6.1 kernels such as Debian 12. The distribution kernel versions come from recall, not a primary source; check them before they are published.
- The sandbox has no mount or PID namespace. A sandboxed process can see other processes and, at ABI 3, signal those of the same user, the builder's process included; Landlock's signal scoping needs ABI 6. Seccomp has to block this, or the helpers have to run where they cannot. Open: no design or test exists yet.
- Landlock does not restrict `chmod`, `chown`, `utime` or `setxattr`. A sandboxed process can change the metadata of files its user owns, outside the workspace and on protected paths. Seccomp has to block these calls, or the reach and protected-path promises must be narrowed. Open: a prototype has to show the gap is closed before the promise is published.
- With network on, the sandbox restricts no destination. Package installs need network on or a builder tool until the proxy exists.
- The agent cannot commit by default. A builder who wants commits changes the protected paths.
- The harness ships a default list of system paths (toolchains, libraries) that a builder can replace. The list is not chosen yet.
- "Safe" covers the built-in tools. A builder tool is the builder's code, and the docs must say so.
- Every built-in file operation crosses into a sandboxed helper process. The cost is unmeasured.
- The `sandbox-exec` man page marks it deprecated. Codex CLI, Claude Code, Cursor and Gemini CLI still use Seatbelt on macOS.
