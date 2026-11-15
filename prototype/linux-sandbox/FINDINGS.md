# Findings: Linux sandbox probe

PROTOTYPE for [#20](https://github.com/jpleva91/harness/issues/20). Run `./run.sh`. Results below are from 2026-11-15 on Ubuntu 24.04, kernel 7.0.0-42, x86-64.

The probe applies Landlock with only the rights ABI 3 has, then a seccomp deny list, then runs a script that tries to exceed the reach [ADR 4](../../docs/adr/0004-sandbox-is-a-fixed-ceiling-built-into-the-core.md) promises.

## Verdicts

| Question | Verdict |
|---|---|
| 1. Can seccomp close the metadata gap? | **No, not usably.** |
| 2. Can seccomp stop signals to the builder? | **Only by breaking job control.** Landlock ABI 6 does it cleanly. |
| 3. Does blocking `socket()` cut the network? | **Yes**, Unix sockets included. Little breaks. |
| 4. Can Landlock keep `.git` read-only in a read-write workspace? | **Not as ADR 4 words it.** Two workable layouts, each with a cost. |
| 5. Spawn cost? | **About 0.5 ms** added per spawn. |

## What held with Landlock alone (ABI 3 rights)

Blocked: reading or creating a file outside reach, hard-linking an outside file in, writing or truncating a read-only path, `ptrace` of an outside process, reading an outside process's `/proc/<pid>/environ`.

## 1. Metadata

With Landlock alone, the sandboxed process could `chmod`, `chown`, set the mtime of, and `setxattr` a file outside its reach that its user owns, and `chmod` a read-only path.

Seccomp cannot filter by path, so the only seccomp fix is to deny these calls everywhere. That blocks the attack and also breaks `touch`, `chmod +x`, `cp -p`, `install -m`, `tar xf` and `git init`/`git commit` inside the workspace.

The gap does not let the agent read or write file contents; Landlock still blocks that. It lets the agent change mode, owner group, times and extended attributes of files the same user owns: `chmod 000` on the builder's files, or `chmod 777` to expose them to other local users.

Not tried: seccomp user notification, where a supervisor checks the path of each call.

## 2. Signals

With Landlock at ABI 3, the sandboxed process signalled a same-user process outside the sandbox.

Denying `kill` and its relatives with seccomp blocks that, and also stops a shell killing its own background job (`sleep 3 & kill $!` fails). `timeout` returned its normal exit code but could not kill its child.

With Landlock at ABI 6 (kernel 6.12) and signal scoping on, the outside signal was blocked and the shell could still kill its own job. ABI 6 also blocked the abstract Unix socket without seccomp.

## 3. Network

With no seccomp, the process created internet and netlink sockets and connected to a Unix socket file outside its reach and to an abstract Unix socket. A Unix socket file is a way out to any daemon the user can reach (Docker, D-Bus).

Denying `socket()` and io_uring blocked all four. Of the tools tried, only Python's `multiprocessing.Queue` failed, and it failed with every seccomp group, so the likely cause is `/dev/shm` being outside reach, not the socket rule.

## 4. Protected paths

Landlock rules only add rights, and a right on a directory covers everything under it. A read-write workspace therefore makes `.git` read-write; the probe wrote `.git/hooks/pre-commit`.

- **Layout B**: workspace read-only, each top-level entry except `.git` read-write. `.git` was protected and existing files and directories worked, but the agent could not create a new top-level file or directory.
- **Layout C**: the git directory lives outside the workspace and is read-only; the workspace holds a `.git` pointer file. Hooks were protected, `git status` worked and top-level files could be created. But the agent could rewrite the `.git` pointer to a directory it controls, so a builder who later runs git in that workspace runs the agent's hooks.

A read-only hole inside a read-write tree needs a mount namespace, which is what bubblewrap provides.

## 5. Cost

300 spawns of `/bin/true`: 0.50 ms bare, 0.97 ms confined (Landlock plus all three seccomp groups). One machine, one run.

## Other things the probe showed

- The reach needs `/dev/null` writable and `/dev/urandom` readable, or git fails. `/proc` was readable in every run.
- `/dev/shm` is shared by every session on the host. Whether it belongs in reach is undecided.

## Limits of this probe

- It ran on kernel 7.0 with rights limited to ABI 3, not on a 6.2 kernel.
- `ptrace` may have been blocked by Yama, not Landlock; the probe does not tell them apart.
- No macOS or Seatbelt work.
- The tool list is short: coreutils, tar, gcc, git, python3, sh.
