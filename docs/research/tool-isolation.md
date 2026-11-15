# Isolation options for tool execution

Research for issue #4. Researched 2026-11-14.

**Question:** What are the options for isolating shell and file tool execution on Linux and macOS? What does each cost in setup and latency? Which ones do existing harnesses use?

**Method:** Primary sources only: kernel docs, man pages, official product docs, and source repositories. Most doc pages were read through a summarizing fetch tool, so wording is paraphrased unless it is quoted. A claim marked **UNVERIFIED** could not be confirmed from a primary source. It is either inference or recall.

## Answer in brief

- The pattern the leading harnesses converge on:
  - **macOS:** Seatbelt.
  - **Linux:** bubblewrap plus seccomp, or Landlock plus seccomp.
  - **Network:** cut at the namespace or profile level, and reachable only through a host-side proxy that enforces a domain allowlist.
  - **Writes:** confined to the workspace and temp dirs, with protected paths (`.git`, agent config, shell rc files) re-masked read-only inside them.
  - **Escapes:** a permission or approval layer decides when a command may run outside the sandbox.
- All of these mechanisms work **unprivileged**, with no daemon. Seatbelt ships with macOS. bwrap is one package. Landlock and seccomp are kernel APIs.
- Containers, gVisor and microVMs give stronger isolation but cost more. Each needs a daemon, KVM, or Apple Virtualization, and adds startup latency. The products that use them, such as OpenHands, Copilot coding agent, E2B and Modal, are cloud-hosted or treat the sandbox as a whole environment rather than a per-command wrapper.
- **No primary source publishes a per-command latency figure** for bwrap, Landlock or Seatbelt. The only first-party numbers are Firecracker's SLA (<=125 ms to guest init) and Daytona's marketing claim (<90 ms).
- The bypasses that keep recurring are policy bugs, not kernel escapes. They involve symlinks, `.git` and worktree path confusion, and sandboxed code writing config files that the host later trusts.

## Comparison of mechanisms

| Mechanism | Restricts | Unprivileged? | Install | Platforms | Startup cost | Main limits |
|---|---|---|---|---|---|---|
| bubblewrap | Filesystem view (mount namespace), PID, network, IPC, UTS; can apply a seccomp filter | Yes, through unprivileged user namespaces (setuid mode removed) | `bubblewrap` package; Codex bundles a copy | Linux | UNVERIFIED; reasoning suggests single-digit to tens of ms | No default policy. Blocked by default on Ubuntu 24.04+ (AppArmor). Bound sockets are escape paths. |
| Landlock | Filesystem access rights; TCP ports (ABI 4); signal and abstract-socket scoping (ABI 6); pathname Unix sockets (ABI 9); UDP (ABI 10) | Yes, with `no_new_privs` | None (kernel 5.13+, LSM enabled) | Linux | UNVERIFIED; in-process, likely microseconds to a few ms | Cannot restrict chdir, stat, chmod, chown and some other calls. `/proc/<pid>/fd` is not restrictable. Features vary by ABI. |
| seccomp-bpf | Syscalls by number and argument value | Yes, with `no_new_privs` | None | Linux | Per-syscall overhead only | Not a filesystem policy. Cannot dereference pointer arguments. x32 and arch pitfalls. Use allow-lists. |
| Seatbelt (`sandbox-exec`) | Filesystem, network, IPC, Mach services, through an SBPL profile | Yes | None (built into macOS) | macOS | UNVERIFIED; reasoning suggests a few to tens of ms | Man page says DEPRECATED. SBPL is undocumented (UNVERIFIED). Profiles break some tools. |
| Rootless Podman / Docker | Full container: namespaces, cgroups, seccomp | Rootless Podman, yes (needs subuid/subgid). Docker usually runs a root daemon. | Container engine, image | Linux native; on macOS a VM (`podman machine`, Docker Desktop) | Container start is the dominant cost (gVisor guide); mitigated by a warm container plus `exec` (UNVERIFIED) | Shared kernel. Bind-mounting the workspace re-exposes it. On macOS the toolchain must run in Linux. |
| gVisor (`runsc`) | Syscalls, intercepted by a userspace kernel | Platforms page does not say (UNVERIFIED); systrap needs no special hardware | `runsc` | Linux only | Higher than runc; most startup cost comes from Docker itself | Syscall-heavy and file-I/O-heavy work is slow. Network stack is less efficient. |
| Firecracker | Whole VM with its own guest kernel | Needs read/write access to `/dev/kvm` | Firecracker, guest kernel and rootfs | Linux with KVM (x86_64, aarch64); no macOS | SLA: <=125 ms to guest `/sbin/init`; <=5 MiB overhead | Needs bare metal or nested-virtualization support. You build the image pipeline yourself. |
| Kata Containers | VM per container | Needs hardware virtualization | Kata runtime | Linux | UNVERIFIED; it boots a guest | Operational weight. |
| Apple `container` / Containerization | Linux VM per container (Virtualization.framework) | Yes, on supported hardware | `container` CLI | macOS 26+, Apple silicon only | "Under a second" (Apple claim, no benchmark) | v0.x with minor-version stability only. Runs Linux binaries, not the macOS toolchain. |
| Lima / other VMs | Whole VM | Yes | Lima | macOS, Linux, Windows | Seconds to boot (UNVERIFIED); keep a long-lived VM | Heavy. File sharing is slow. |

### Evidence by mechanism

**bubblewrap.** [README](https://github.com/containers/bubblewrap/blob/main/README.md)

- It builds a new mount namespace on a tmpfs root, and only the paths you bind in are visible.
- It can unshare user, IPC, PID, net and UTS namespaces, apply a seccomp filter, and set `NO_NEW_PRIVS`.
- The setuid mode has been removed, so it depends on unprivileged user namespaces.
- It is a toolkit with no default policy. Without TIOCSTI filtering it needs `--new-session` (CVE-2017-5226). A bound D-Bus socket or other socket can be an escape path.
- Ubuntu 23.10 introduced the AppArmor restriction on unprivileged user namespaces as opt-in ([blog](https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces)). Ubuntu 24.04 turns it on for unconfined programs ([release notes](https://discourse.ubuntu.com/t/ubuntu-24-04-lts-noble-numbat-release-notes/39890)). The workaround is an AppArmor profile with a `userns` rule, or a sysctl. This is a real install-friction point for a library that embeds bwrap.

**User namespaces.** [user_namespaces(7)](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)

- Unprivileged processes can create them since Linux 3.8, with `CONFIG_USER_NS`.
- Capabilities granted inside a namespace apply only inside it.
- Ubuntu's argument for the restriction is that user namespaces expand the kernel attack surface. They are not a boundary against kernel bugs.

**Landlock.** [kernel doc](https://docs.kernel.org/userspace-api/landlock.html)

- It needs `no_new_privs`, `CONFIG_SECURITY_LANDLOCK`, and landlock in the active LSM list.
- Probe for support with `landlock_create_ruleset(NULL,0,LANDLOCK_CREATE_RULESET_VERSION)`. ENOSYS means unsupported; EOPNOTSUPP means disabled at boot.
- ABI list:
  1. Filesystem (kernel 5.13)
  2. REFER (cross-directory rename and link)
  3. TRUNCATE
  4. TCP bind and connect by port
  5. IOCTL_DEV
  6. Scoping of abstract Unix sockets and signals
  7. Audit flags
  8. TSYNC
  9. RESOLVE_UNIX (pathname Unix sockets)
  10. UDP rights
  11. NO_NEW_PRIVS flag
  12. Capability and namespace rules
- The doc maps only ABI 1 to a kernel version. The mapping of ABI 4 to kernel 6.7 is from recall (UNVERIFIED), and the later mappings are unknown.
- Cursor's docs require kernel 6.2+ with Landlock v3 ([run modes](https://cursor.com/docs/agent/security/run-modes)).
- Limits: chroot is not denied. `/proc/<pid>/fd` and `/proc/<pid>/ns` are not restrictable. At most 16 stacked layers. chdir, stat, chmod, chown, flock, setxattr, utime, fcntl and access cannot be restricted.

**seccomp-bpf.** [seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)

- A filter sees only a snapshot of six register-sized arguments, so pointer arguments cannot be inspected.
- The filter must check the architecture. On x86-64 the x32 ABI can bypass deny-lists. vDSO calls never reach the filter. Allow-lists are preferred.
- Limits are 4096 instructions per filter and 32768 in total.
- TOCTOU on pointed-to data is a known class but is not stated in the man page (UNVERIFIED there).
- Seccomp works as a complement to bwrap or Landlock, not on its own. Codex uses it to block io_uring, AF_VSOCK and network syscalls. sandbox-runtime uses it to block creating Unix sockets.

**Seatbelt.** [sandbox-exec(1)](https://keith.github.io/xcode-man-pages/sandbox-exec.1.html)

- The man page says "The sandbox-exec command is DEPRECATED" and points to App Sandbox.
- Codex, Claude Code (through sandbox-runtime), Cursor and Gemini CLI all still use it. Cursor's [engineering post](https://cursor.com/blog/agent-sandboxing) explains why: App Sandbox needs signed binaries, and containers or VMs only run Linux binaries, which rules out the user's macOS toolchain.
- Profiles are generated at runtime from workspace settings.
- sandbox-runtime documents profile weakeners ([README](https://github.com/anthropic-experimental/sandbox-runtime)): `allowLocalBinding`, a trustd exfiltration path under `enableWeakerNetworkIsolation`, and `allowAppleEvents`.

**Containers.**

- Rootless Podman needs `/etc/subuid` and `/etc/subgid` and `pasta` for networking. Rootless overlayfs needs kernel 5.12.9+ or fuse-overlayfs, and NFS home directories are unsupported ([podman(1)](https://docs.podman.io/en/latest/markdown/podman.1.html)).
- On macOS Podman uses `podman machine`, which is a VM.
- That Docker Desktop on macOS runs a VM is common knowledge, but the fetched install page did not state it (UNVERIFIED there).

**gVisor.** [platforms](https://gvisor.dev/docs/user_guide/platforms/), [performance](https://gvisor.dev/docs/architecture_guide/performance/)

- systrap has been the default platform since mid-2023. The KVM platform needs `/dev/kvm`. ptrace is deprecated.
- Syscall overhead is the main structural cost. File I/O has known serialization points in the VFS. CPU and memory access are near native.
- Modal's Sandboxes use gVisor by default ([docs](https://modal.com/docs/guide/sandbox)).

**Firecracker.** [getting started](https://github.com/firecracker-microvm/firecracker/blob/main/docs/getting-started.md), [SPECIFICATION.md](https://github.com/firecracker-microvm/firecracker/blob/main/SPECIFICATION.md)

- It needs KVM and runs on x86_64 or aarch64 Linux. On EC2 that means .metal instances.
- SLA: about 12 ms wall-clock for the VMM to reach its API socket, <=125 ms from InstanceStart to guest init, and <=5 MiB memory overhead.
- E2B describes its sandboxes as an on-demand Linux VM ([docs](https://docs.e2b.dev/)). The docs page does not name Firecracker (UNVERIFIED).

**Kata Containers.** [README](https://github.com/kata-containers/kata-containers/blob/main/README.md)

- It runs a VM per container and needs VT-x, SVM or an equivalent.

**Apple `container`.** [apple/container](https://github.com/apple/container), [apple/containerization](https://github.com/apple/containerization)

- It needs macOS 26+ on Apple silicon and runs one lightweight VM per container on Virtualization.framework.
- It is at 0.x, and stability is guaranteed only within a minor version.

**Windows (for later).**

- AppContainer isolates files, registry, network and processes ([MS docs](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)).
- sandbox-runtime's Windows support is alpha. It runs commands as a dedicated local user with a write-restricted token, a WFP network filter and NTFS ACEs, and needs a one-time elevated install. Codex's Windows sandbox uses a similar restricted-token, ACL and WFP design (inferred from file names in `codex-rs/windows-sandbox-rs`, UNVERIFIED in detail).
- Cursor runs its Linux sandbox inside WSL2 on Windows.

## What existing harnesses use

| Harness | Linux | macOS | Default | Network | Source |
|---|---|---|---|---|---|
| Claude Code | bubblewrap + socat + seccomp (sandbox-runtime) | Seatbelt (generated profile) | **Off**, opt-in with `/sandbox`. Missing deps fall back to running unsandboxed unless `failIfUnavailable` is set. | No direct route out. Host proxy (HTTP and SOCKS5) with a domain allowlist that is empty by default. No UDP. | [docs](https://code.claude.com/docs/en/sandboxing), [sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime) |
| Codex CLI | bubblewrap + seccomp. Landlock is now legacy. | Seatbelt (`.sbpl` profiles) | **On**: workspace-write in git repos, read-only elsewhere until trusted | Off by default in workspace-write. Optional allowlist proxy through `--unshare-net` and a Unix-socket bridge. | [linux-sandbox README](https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/README.md), [sandboxing docs](https://learn.chatgpt.com/docs/sandboxing) |
| Cursor agent | Landlock + seccomp, with an overlay that hides ignored files | Seatbelt (generated profile) | **On** in Auto-review mode. Falls back to approval if the kernel lacks Landlock v3. | Blocked by default, with an allowlist set in `sandbox.json` | [blog](https://cursor.com/blog/agent-sandboxing), [run modes](https://cursor.com/docs/agent/security/run-modes) |
| Gemini CLI | Whole-process container (Docker, Podman, gVisor, LXC). A newer per-tool bwrap manager exists in `packages/core/src/sandbox`. | Seatbelt profiles; the default is `permissive-open` | **Opt-in** (`-s`, `GEMINI_SANDBOX`) | The default profile allows network. Proxied profiles exist. | [sandbox.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/sandbox.md), [LinuxSandboxManager.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/sandbox/linux/LinuxSandboxManager.ts) |
| OpenHands | Docker runtime | Docker | **On**. The `process` runtime is documented as "unsafe, but fast". | Not documented on the page read (UNVERIFIED) | [sandboxes overview](https://docs.openhands.dev/openhands/usage/sandboxes/overview) |
| Copilot coding agent | GitHub Actions runner or cloud sandbox (an ephemeral VM) | n/a | **On** (cloud-only) | Firewall allowlist, which covers Bash-tool processes but not other MCP servers | [about](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent), [firewall](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/customize-the-agent-firewall) |
| OpenCode | None. Allow, ask and deny permission rules, mostly allow by default. | None | n/a | Host network | [permissions](https://opencode.ai/docs/permissions/), [issue #48411](https://github.com/anomalyco/opencode/issues/48411) |
| Goose | None. An experimental macOS Seatbelt sandbox was removed. | None | n/a | Host network | [issue #12805](https://github.com/aaif-goose/goose/issues/12805), [issue #10895](https://github.com/aaif-goose/goose/issues/10895) |
| Aider, Cline, Roo Code, Zed | None. Confirmations, auto-approve toggles, or prefix and regex lists. | None | n/a | Host network | [aider options](https://aider.chat/docs/config/options.html), [cline](https://docs.cline.bot/features/auto-approve), [roo](https://roocodeinc.github.io/Roo-Code/features/auto-approving-actions), [zed](https://zed.dev/docs/ai/tool-permissions) |

### Design details worth copying

- **The sandbox and approvals are separate axes.** Codex's docs put it this way: the sandbox sets what is technically possible, and the approval policy sets when to ask. Claude Code and Cursor both let a failing sandboxed command be retried outside the sandbox, gated by a prompt, a classifier, or a deny, depending on the mode.
- **Protected paths inside writable roots.**
  - Codex re-mounts `.git`, the resolved gitdir and `.codex` read-only, and masks symlinks and not-yet-existing protected paths with `/dev/null`.
  - Claude Code write-denies `.claude` settings, hooks and skills, `.mcp.json`, shell rc files, `.gitconfig` and `.git/hooks`.
  - Cursor protects `.git/config`, `.git/hooks`, `.vscode` and `.cursorignore`.
- **Nested user namespaces are blocked.** sandbox-runtime blocks them because a private mount tree would let the process undo the write-deny binds.
- **Network through a Unix-socket bridge.** The network namespace is removed entirely, and the host proxy is reached over a bind-mounted Unix socket. Seccomp stops the process from creating other Unix sockets. Both Codex and sandbox-runtime do this.
- **The sandbox usually covers only the shell tool.** Claude Code's built-in Read, Edit and Write tools are governed by permission rules, not by the OS sandbox. Hooks, local MCP servers and LSP servers also run outside it. A harness that ships its own file tools has to decide whether those run in-process (policy checks) or in the sandbox.
- **A configurable fallback when the sandbox is unavailable.** Claude Code fails open unless `failIfUnavailable` is set. Cursor falls back to asking for approval. For a builder-facing library, failing closed by default seems the safer choice. That is inference, not a sourced recommendation.

### Known bypasses (advisories)

- Claude Code / sandbox-runtime:
  - [GHSA-9gqj-5w7c-vx47](https://github.com/anthropics/sandbox-runtime/security/advisories/GHSA-9gqj-5w7c-vx47): the network sandbox was not enforced when no allowed domains were configured.
  - [GHSA-ff64-7w26-62rf](https://github.com/anthropics/claude-code/security/advisories/GHSA-ff64-7w26-62rf): sandboxed code could create a `.claude/settings.json` that did not exist at startup, and its hooks ran on the host.
  - [GHSA-vp62-r36r-9xqp](https://github.com/anthropics/claude-code/security/advisories/GHSA-vp62-r36r-9xqp): symlink following allowed writes outside the workspace.
  - [GHSA-7835-87q9-rgvv](https://github.com/anthropics/claude-code/security/advisories/GHSA-7835-87q9-rgvv): git worktree path confusion plus fsmonitor overwrote `~/.zshenv`.
  - [GHSA-5j29-h97v-84ch](https://github.com/anthropics/claude-code/security/advisories/GHSA-5j29-h97v-84ch): a write-time symlink TOCTOU allowed arbitrary file writes.
- Codex:
  - [GHSA-w5fx-fh39-j5rw](https://github.com/openai/codex/security/advisories/GHSA-w5fx-fh39-j5rw): a model-supplied `cwd` became the writable root.
- Goose:
  - [issue #12566](https://github.com/aaif-goose/goose/issues/12566): a prompt-injection RCE report against the unsandboxed shell tool.

Every advisory above is a policy or path bug, not a kernel-primitive escape. The hard part of the design is the policy: which paths are writable, how symlinks are resolved, and which files the host later trusts.

## Implications for the harness decision

These are inferences for the later ADR, not sourced facts.

1. **The default tier that runs without privileges and needs no daemon:**
   - **macOS:** Seatbelt.
   - **Linux:** bwrap plus seccomp, with Landlock as a fallback or as a complement.
   - **Network:** a host-side allowlist proxy.
2. **Ubuntu 24.04's AppArmor userns restriction is the biggest install risk on Linux.** The harness needs a probe, a clear error, and a documented AppArmor profile, or a Landlock-only fallback (Cursor's approach).
3. **Containers and microVMs belong in a pluggable "remote or heavy executor" interface**, not in the default path. Use cases are cloud agents, untrusted multi-tenant runs, and the E2B/Modal/Daytona class of backends.
4. **Measure per-command latency ourselves.** No primary source gives numbers for bwrap or Seatbelt.

## Not verified

- Per-command latency for bwrap, Landlock, Seatbelt and warm-container exec.
- Landlock ABI-to-kernel mapping beyond ABI 1.
- Codex Windows internals and Seatbelt profile contents.
- The Gemini macOS and Windows managers.
- OpenHands network and mount policy.
- Aider's confirmation flow for suggested commands.
- Amp CLI permissions.
- Whether Docker's default seccomp profile is applied under every engine.
- E2B's underlying VMM.
