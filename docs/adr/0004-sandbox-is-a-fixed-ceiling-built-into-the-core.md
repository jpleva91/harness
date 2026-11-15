# 4. The sandbox is a fixed ceiling built into the core

Date: 2026-11-15. Status: accepted. Decided in [#11](https://github.com/jpleva91/harness/issues/11), amended the same day after the probe in [#20](https://github.com/jpleva91/harness/issues/20) ([findings](https://github.com/jpleva91/harness/blob/prototype/linux-sandbox/prototype/linux-sandbox/FINDINGS.md)).

## Context

The wedge is the safe in-process harness ([#5](https://github.com/jpleva91/harness/issues/5)): deny unless allowed, sandbox on, network off. The isolation research ([#4](https://github.com/jpleva91/harness/issues/4)) found that existing harnesses let an approval lift a call out of the sandbox, run their file tools in-process behind path checks, and fall back to no sandbox when a dependency is missing. The advisories reviewed in #4 were all policy or path bugs, not kernel escapes. On Ubuntu 24.04 AppArmor blocks bubblewrap by default.

## Decision

Permissions decide whether a tool call starts. The sandbox bounds what a started call can touch. The two do not trade against each other.

- The sandbox is set in the session's config and is always on for the built-in shell and file tools. No permission rule and no approval widens it. To widen it, the builder changes the session's config.
- The built-in file tools run under the same sandbox as the shell tool, not in-process behind path checks.
- The sandbox is built into the core with no external binary: Landlock plus seccomp on Linux, Seatbelt on macOS.
- The Linux floor is Landlock ABI 6, kernel 6.12. Below ABI 6 Landlock cannot stop a sandboxed process signalling the builder's process.
- Landlock does not control socket creation, so seccomp cuts the network by denying `socket()` and io_uring. That also closes Unix socket files, which Landlock does not cover below ABI 9.
- If the sandbox cannot be set up, creating the session fails. There is no unsandboxed fallback.
- Default reach: read and write in the workspace and a private temp directory, read-only on the system paths the builder lists, nothing else. Reach is a promise about file contents, not metadata.
- There are no protected paths inside the workspace in v1. The whole workspace is the agent's, and the builder treats it as untrusted.
- Network is off by default. One switch on the session turns it fully on.
- A call the sandbox refuses is a tool failure, which goes back to the model.
- Builder tools run in the builder's process, outside the sandbox, and are governed by permission rules only.

## Alternatives rejected

- **An approval lifts a call out of the sandbox.** Codex does this. A prompt-injected agent then needs one mistaken approval.
- **A sandbox the builder can switch off.** Convenient inside a container, but it makes running unsandboxed possible by accident.
- **bubblewrap first.** Stronger (mount and PID namespaces), but it is an external or bundled binary and needs an AppArmor profile on Ubuntu 24.04.
- **A bubblewrap fallback on older kernels.** Widest reach, but two Linux sandboxes to build and test in v1.
- **A floor of ABI 3, kernel 6.2.** The first choice. The probe showed a sandboxed process signalling a process outside, and denying `kill` with seccomp broke shell job control.
- **Signal scoping only where the kernel has it.** The guarantee would differ by host.
- **Denying `chmod`, `chown`, `utime` and `setxattr` with seccomp.** It closes the metadata gap and breaks `touch`, `chmod +x`, `cp -p`, `tar` and `git commit`.
- **`.git` read-only inside the workspace.** Landlock rights only add and cover everything under a directory. The layouts that work either stop the agent creating top-level entries or leave a `.git` pointer the agent can rewrite.
- **File tools in-process.** Faster, but a path or symlink bug then reads the builder's own files.
- **A domain-allowlist proxy in v1.** It fits the model later without changing it, and it is a sizeable component.

## Consequences

- Hosts the harness refuses: any kernel below 6.12, which includes RHEL 8 and 9, Debian 12, and Ubuntu 22.04 and 24.04 on their original kernels. The distribution kernel versions come from recall, not a primary source; check them before they are published.
- The sandbox has no mount or PID namespace. A sandboxed process can see other processes. In the probe it could not signal a process outside its sandbox or read that process's environment; it could still signal its own children. Tracing was blocked too, but the probe did not tell Landlock from Yama.
- A sandboxed process can change the mode, owner group, times and extended attributes of files its user owns outside its reach. It cannot read or write their contents. Sessions in one process share a user, so one session can change the metadata of another session's files, for example `chmod 000`. The docs must say so and tell builders to run the harness as a user that owns nothing else. A supervisor that checks the path of each call is a question for after v1.
- The builder must not run git hooks or trust config from a workspace. A builder who needs a trusted git directory keeps it outside the workspace.
- A confined spawn cost about 0.5 ms more than a bare one in the probe, on one machine.
- With network on, the sandbox restricts no destination. Package installs need network on or a builder tool until the proxy exists.
- The agent can commit in the workspace.
- The harness ships a default list of system paths (toolchains, libraries) that a builder can replace. The list is not chosen yet. The probe needed `/dev/null` writable and `/dev/urandom` readable; whether the shared `/dev/shm` belongs in reach is undecided.
- "Safe" covers the built-in tools. A builder tool is the builder's code, and the docs must say so.
- Every built-in file operation crosses into a sandboxed helper process. The cost of that crossing is unmeasured.
- The `sandbox-exec` man page marks it deprecated. Codex CLI, Claude Code, Cursor and Gemini CLI still use Seatbelt on macOS.
