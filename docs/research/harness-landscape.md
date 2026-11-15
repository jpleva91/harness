# Landscape of existing agent harnesses

Research for [issue #2](https://github.com/jpleva91/harness/issues/2). As of 2026-11-14.

**Question.** How do existing agent-loop harnesses structure the loop, tools, context management, permissions and embedding surface? What is painful or missing for an agent builder who embeds one today?

**Method and confidence.**
- **Sources.** Official docs, source READMEs and GitHub issue trackers, read through `gh` and web fetch. Every claim links to its source.
- **Issue citations** are evidence of *reported* pain. Most were judged by title and summary. Their bodies were not read in full, and they were not checked for whether they are still current.
- **UNVERIFIED** marks a claim I could not confirm from a primary source. "Absent" claims (for example "no deterministic replay") come from not finding the feature, not from an exhaustive search, so they are UNVERIFIED as absences.
- **Spot checks.** I checked the Claude Agent SDK hosting page directly, and I checked that vercel/ai#24624, aaif-goose/goose#11778 and openai/codex#42313 exist with the titles described.

## TL;DR

1. **The embeddable coding harnesses are mostly subprocesses or servers, not libraries.**
   - The Claude Agent SDK spawns a bundled `claude` CLI per session.
   - Codex's SDKs drive a Rust app-server over JSON-RPC.
   - OpenCode embeds as a local HTTP server.
   - pi is the only coding-first harness that runs in-process as a library. The OpenAI Agents SDK, Vercel AI SDK and Pydantic AI also run in-process, but none of them is coding-first.
   - Most open embedding bugs sit at the process boundary: leaked children, framing, pipes, lifecycle.
2. **Provider neutrality and coding quality don't coexist.**
   - The strongest coding harnesses are tied to one vendor. Claude is Claude-only. In Codex and the OpenAI Agents SDK, custom providers lose compaction, replay or features, sometimes silently.
   - The neutral ones (pi, OpenCode, Vercel AI SDK, Pydantic AI, Deep Agents) leave sandboxing, permissions or durable state to the builder.
3. **No exec sandbox in-library except Codex.**
   - Codex ships Seatbelt, bwrap + seccomp, and Windows sandboxes.
   - Everyone else says "use a container" (Claude, OpenAI Agents SDK, pi, Deep Agents), is experimental or removed (Goose), or has a bespoke interpreter as the boundary with a reported escape (smolagents).
4. **Compaction, durable resume, and approval-across-resume are the shared bug magnets.** Content-free summaries, dropped images, and races with turns. Re-executed approved calls after a crash. Pending approvals lost on restore. One unknown event kind that breaks session load.
5. **No harness offers a first-class deterministic replay or mock-model seam** (UNVERIFIED as absent). There is no shared event or trace schema. ACP is the only cross-vendor event and permission vocabulary, and it covers only the agent-to-editor edge.
6. **Multi-tenant isolation is opt-out and leaky.** Claude reads home-directory state unless you lock it down. Codex has env and identity leak issues. Subagents bypass parent approvals in Goose and OpenCode.

## Comparison at a glance

| Harness | Embed surface | Coding tools | Exec sandbox | Providers | License |
|---|---|---|---|---|---|
| Claude Agent SDK | Subprocess (`claude` CLI over stdio) | Yes, full | No, "use a container" | Anthropic (API, Bedrock, Vertex) | SDK MIT; Claude Code proprietary, Commercial Terms |
| Codex | App-server JSON-RPC; TS and Python SDKs wrap it | Yes | Yes, OS-level | OpenAI first; custom providers second-class | Apache-2.0 |
| OpenAI Agents SDK | In-process library | Via "sandbox agents" | Docker or hosted recommended | OpenAI first; others via degraded paths | MIT |
| OpenCode | Local HTTP server + generated SDK | Yes | None | Many (models.dev catalog) | MIT |
| pi | In-process TypeScript library | Yes | None, "use a container or VM" | ~35 provider entries | MIT |
| Aider | Unsupported Python API | Edit formats, no tool loop | None documented | Many, via litellm | Apache-2.0 |
| Goose | ACP; Rust; alpha SDK | Via MCP extensions | Removed or prototype | 15+ (claimed) | Apache-2.0 |
| smolagents | In-process Python | Code-as-action | AST interpreter or remote executors | Many | Apache-2.0 |
| OpenHands SDK | Python SDK + REST/WebSocket agent server | Yes | Docker or Kubernetes workspaces | Any LLM (claimed) | MIT |
| Vercel AI SDK | In-process TypeScript | No | Experimental | Many | UNVERIFIED |
| Pydantic AI | In-process Python | No | No | Many | UNVERIFIED |
| Deep Agents (LangGraph) | In-process Python and TypeScript | Yes | Bring your own | Any tool-calling model | MIT |

---

## 1. Claude Agent SDK and Claude Code

### Loop
- It is the "same execution loop that powers Claude Code" ([agent-loop](https://code.claude.com/docs/en/agent-sdk/agent-loop.md)).
- **Messages.** It yields typed messages: `SystemMessage(init)`, `AssistantMessage`, `UserMessage` (tool results), `StreamEvent` and `ResultMessage`.
- **Stop conditions.**
  - The loop ends when the model makes no tool calls.
  - `max_turns` and `max_budget_usd` both default to unlimited.
  - Result subtypes are `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution` and `error_max_structured_output_retries`.
  - `query()` raises after yielding an error result ([hosting](https://code.claude.com/docs/en/agent-sdk/hosting.md)).
- **Subagents** run through the `Agent` tool plus `AgentDefinition`. The `usage` field covers only the main loop, so whole-tree accounting needs `modelUsage` ([agent-loop](https://code.claude.com/docs/en/agent-sdk/agent-loop.md)).

### Tools
- **Built-in.** Read, Edit, Write, Glob, Grep, Bash, WebSearch, WebFetch, ToolSearch, Agent, Skill, AskUserQuestion and Task*.
- **MCP** is supported, with deferred tool loading.
- **Custom tools** are in-process MCP servers (`@tool`, `create_sdk_mcp_server`). In Python, hooks and custom tools require `ClaudeSDKClient` ([python README](https://raw.githubusercontent.com/anthropics/claude-agent-sdk-python/main/README.md)).
- **No SDK-native exec sandbox was found.** The docs point to Docker, gVisor or Firecracker ([secure-deployment](https://code.claude.com/docs/en/agent-sdk/secure-deployment.md)). Whether Claude Code's own sandbox is exposed through the SDK is UNVERIFIED.

### Context
- **Auto-compaction** emits a `compact_boundary`. A `PreCompact` hook and CLAUDE.md summary instructions customize it.
- **Prompt caching** covers the system prompt, tools and CLAUDE.md ([agent-loop](https://code.claude.com/docs/en/agent-sdk/agent-loop.md)).
- **Sessions** are JSONL on local disk, written by the subprocess.
  - Resume, fork and continue all go by session ID.
  - A `SessionStore` mirrors transcripts to your backend. The mirror is best-effort, and failed batches are dropped with `mirror_error`.
  - Memory files and working-directory artifacts are not mirrored ([hosting](https://code.claude.com/docs/en/agent-sdk/hosting.md)).

### Permissions
- **Modes:** `default`, `acceptEdits`, `plan`, `dontAsk`, `auto` (a classifier) and `bypassPermissions`.
- **`canUseTool`** is a callback for anything not pre-approved.
- **Hooks:** PreToolUse, PostToolUse, Stop, SubagentStart/Stop and PreCompact ([agent-loop](https://code.claude.com/docs/en/agent-sdk/agent-loop.md)).
- **Footgun:** `allowed_tools` auto-approves the listed tools but does not restrict anything. Blocking needs `disallowed_tools` ([python README](https://raw.githubusercontent.com/anthropics/claude-agent-sdk-python/main/README.md)).

### Embedding
- **Process model.** "The Agent SDK spawns and supervises a `claude` CLI subprocess". One session is one subprocess. The bundled native binary is pinned to the SDK version ([hosting](https://code.claude.com/docs/en/agent-sdk/hosting.md)).
- **Resources.** The suggested starting point is 1 GiB RAM, 1 CPU and 5 GiB disk per agent, and memory grows with session length.
- **Known limitations** (from the docs):
  - No session timeout.
  - No per-subagent deadline.
  - Rate limits on wide fanouts.
- **Alternative.** Anthropic also offers hosted [Managed Agents](https://platform.claude.com/docs/en/managed-agents/overview), where the loop runs on their side.
- **Licensing.**
  - The SDK repo is MIT ([repo](https://github.com/anthropics/claude-agent-sdk-python)).
  - Claude Code itself is "All rights reserved", under Commercial Terms ([LICENSE](https://github.com/anthropics/claude-code/blob/main/LICENSE.md)).
  - Branding rules apply, and third parties may not offer claude.ai login ([overview](https://code.claude.com/docs/en/agent-sdk/overview)).

### Builder pain
- **Multi-tenant isolation is opt-out.** You must set `settingSources: []`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` (auto memory loads "regardless of `settingSources`"), a per-tenant `CLAUDE_CONFIG_DIR` and `cwd`, and per-tenant egress rules ([hosting](https://code.claude.com/docs/en/agent-sdk/hosting.md)).
- **Process-boundary bugs:**
  - CLI child leaked on setup failure ([py#1514](https://github.com/anthropics/claude-agent-sdk-python/issues/1514)).
  - Malformed frames kill the read loop ([py#1515](https://github.com/anthropics/claude-agent-sdk-python/issues/1515)).
  - Control-request leaks ([py#1516](https://github.com/anthropics/claude-agent-sdk-python/issues/1516)).
  - Usage and stream events missing in 20-70% of concurrent chats ([py#435](https://github.com/anthropics/claude-agent-sdk-python/issues/435)).
  - Parent session env leaks into the child ([ts#520](https://github.com/anthropics/claude-agent-sdk-typescript/issues/520)).
- **Sessions:**
  - Resume problems with `SessionStore` ([py#1387](https://github.com/anthropics/claude-agent-sdk-python/issues/1387), [py#1521](https://github.com/anthropics/claude-agent-sdk-python/issues/1521)).
  - No access to pre-compaction history ([py#1454](https://github.com/anthropics/claude-agent-sdk-python/issues/1454)).
  - Forks miss the prompt cache ([ts#528](https://github.com/anthropics/claude-agent-sdk-typescript/issues/528)).
- **Correctness:**
  - Tool `isError` reported as success ([py#1492](https://github.com/anthropics/claude-agent-sdk-python/issues/1492)).
  - `total_cost_usd` omits interrupted turns ([ts#522](https://github.com/anthropics/claude-agent-sdk-typescript/issues/522)).
- **OTel spans** don't follow the GenAI semantic conventions ([py#1537](https://github.com/anthropics/claude-agent-sdk-python/issues/1537)).
- **Supply chain.** Bundled dependencies are invisible to `npm audit` ([ts#530](https://github.com/anthropics/claude-agent-sdk-typescript/issues/530)).

## 2. OpenAI Codex (CLI, `codex exec`, app-server, SDKs)

### Loop and surface
- **Codebase.** Apache-2.0 Rust workspace: core, exec, app-server, app-server-protocol, bwrap, codex-mcp and others ([tree](https://github.com/openai/codex/tree/main/codex-rs)).
- **Transports.** The app-server speaks JSON-RPC over stdio, in-process or WebSocket ([app-server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)).
- **Model.** Threads, turns and items.
  - Thread methods: `thread/start|resume|fork|archive|revert`.
  - `turn/start` begins a turn, and turns end with `turn/completed`.
- **Churn.** Many features sit behind `experimentalApi: true`. Older servers silently ignore newer fields. `thread/rollback` was removed in favour of `thread/revert`.
- **SDKs.**
  - TypeScript: `@openai/codex-sdk` (Node 18+).
  - Python: `openai-codex`, which "controls the local Codex app-server over JSON-RPC" and ships a pinned CLI ([SDK docs](https://learn.chatgpt.com/docs/codex-sdk)).
  - The transport inside the TS SDK and the loop's stop-condition details are UNVERIFIED.

### Tools and sandbox
- **Sandbox modes:** `read-only`, `workspace-write` and `danger-full-access`. Approval policies are configured separately.
- **Platforms:** Seatbelt on macOS, bwrap + seccomp on Linux, and a Windows sandbox.
- **Containers.** Docker and devcontainers may block the namespaces the sandbox needs, which leaves the container as the only boundary.
- **Network** is off by default with a proxy allowlist. `.git`, `.codex` and similar directories are read-only ([approvals-security](https://learn.chatgpt.com/docs/agent-approvals-security)).
- **This is the only harness in this survey with a built-in OS sandbox.**

### Context
- **AGENTS.md** is loaded from the cwd up to the project root, capped by `project_doc_max_bytes`.
- **Project config** in `.codex/` loads only for trusted projects.
- **Custom providers** are defined under `[model_providers.<id>]`.
- **OTel** is opt-in ([config-advanced](https://learn.chatgpt.com/docs/config-file/config-advanced)).
- **Threads** persist, with resume and fork.

### Permissions
- **Approvals.** Command approvals arrive as `item/commandExecution/requestApproval`. With no capable client connected the command is rejected. Pending prompts replay on reconnect ([app-server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)).
- **Hooks** exist. Details are UNVERIFIED.

### Builder pain
- **Non-OpenAI providers are second-class:**
  - The custom provider's display name silently controls remote compaction ([#42313](https://github.com/openai/codex/issues/42313)).
  - Resume or handoff to third-party Responses providers fails on the "compaction" item ([#45393](https://github.com/openai/codex/issues/45393)).
  - Local providers can't auto-compact ([#48870](https://github.com/openai/codex/issues/48870)).
  - A custom-provider 401 touches ChatGPT credentials ([#60468](https://github.com/openai/codex/issues/60468)).
- **Process lifecycle:**
  - The Python SDK leaks the app-server on a startup timeout ([#46903](https://github.com/openai/codex/issues/46903)).
  - Pipes stay open after close ([#54908](https://github.com/openai/codex/issues/54908)).
  - A full stdin queue blocks kill ([#60906](https://github.com/openai/codex/issues/60906)).
  - TS item types have drifted from the wire format ([#53017](https://github.com/openai/codex/issues/53017)).
- **The event stream is not a complete record:**
  - Code-mode exec is not a thread item ([#60694](https://github.com/openai/codex/issues/60694)).
  - MCP outcomes are omitted ([#55926](https://github.com/openai/codex/issues/55926)).
  - Sandbox-denied commands emit no items ([#60470](https://github.com/openai/codex/issues/60470)).
- **Isolation leaks:**
  - App-server env reaches `exec_command` despite `include_only` ([#48096](https://github.com/openai/codex/issues/48096)).
  - The exec-server falls back to the host identity ([#41563](https://github.com/openai/codex/issues/41563)).
- **Surfaces diverge.** The app-server and `codex exec` behave differently ([#60133](https://github.com/openai/codex/issues/60133), [#49827](https://github.com/openai/codex/issues/49827)).

## 3. OpenAI Agents SDK (Python and TypeScript)

### Loop
- **Runner cycle.** The Runner calls the LLM, then:
  1. ends on a final output,
  2. switches agent on a handoff, or
  3. runs tools and loops.
- **Turn limits.** `max_turns` raises `MaxTurnsExceeded`, which `error_handlers` can turn into a fallback.
- **Pause and resume.** A run can pause for tool approval and resume through a serializable `RunState`. `run_streamed()` gives the event stream ([running_agents](https://openai.github.io/openai-agents-python/running_agents/)).
- **Primitives:** handoffs, agents-as-tools, guardrails, sessions and tracing.
- **License:** MIT ([repo](https://github.com/openai/openai-agents-python)).
- **This is a true in-process library.**

### Tools and sandbox
- **Tool types:** function, hosted and MCP tools.
- **Coding tools.** There is no default coding toolset. "Sandbox agents" add shell, filesystem, skills, memory and compaction.
- **Local confinement is weak.** `UnixLocalSandboxClient` has no OS confinement on Linux. On macOS it uses `sandbox-exec` without network isolation. The docs recommend Docker or a hosted provider for untrusted code ([sandbox_agents](https://openai.github.io/openai-agents-python/sandbox_agents/)).

### Context
- **Sessions** are pluggable.
- **No general compaction:**
  - `nest_handoff_history` is beta.
  - `call_model_input_filter` lets you rewrite the model input.
  - The TS SDK has `OpenAIResponsesCompactionSession` ([JS docs](https://openai.github.io/openai-agents-js/)).
- **Conflict.** Sessions can't be combined with `conversation_id` or `previous_response_id`.

### Providers
- **Paths.** Non-OpenAI models run through a `ModelProvider`, an OpenAI-compatible client, or beta LiteLLM and any-llm adapters.
- **Silent feature loss on Chat Completions:**
  - Responses-only fields are silently dropped unless `strict_feature_validation=True`.
  - Responses-only tools are rejected.
  - Streamed tool calls can be unreliable.
- **Tracing** defaults to OpenAI's backend ([models](https://openai.github.io/openai-agents-python/models/)).

### Builder pain
- **Approval and resume:**
  - A restored session hides a pending approval, so the run waits forever ([#5995](https://github.com/openai/openai-agents-python/issues/5995)).
  - An approval transfers across runs on a reused call ID ([#5413](https://github.com/openai/openai-agents-python/issues/5413)).
  - A `RunState` round-trip duplicates handoff summaries ([#5678](https://github.com/openai/openai-agents-python/issues/5678)).
- **History left in states the API rejects:**
  - Windowing splits a reasoning item ([#5507](https://github.com/openai/openai-agents-python/issues/5507)).
  - `max_turns` leaves tool calls without outputs ([#5642](https://github.com/openai/openai-agents-python/issues/5642)).
- **Provider handling:**
  - LiteLLM with Anthropic drops `strict` ([#5825](https://github.com/openai/openai-agents-python/issues/5825)).
  - `max_tokens` is sent to reasoning models ([#5455](https://github.com/openai/openai-agents-python/issues/5455)).
- **Delegation:** no recursion budget for `as_tool` delegation ([#5453](https://github.com/openai/openai-agents-python/issues/5453)).
- **Unverified:** TypeScript SDK parity with Python.

## 4. OpenCode (now `anomalyco/opencode`, MIT, TypeScript)

The project was formerly `sst/opencode` ([repo](https://github.com/anomalyco/opencode)).

### Loop
- **Client/server.** `opencode serve` runs an HTTP server on 127.0.0.1:4096, and the TUI is one client ([server](https://opencode.ai/docs/server/)).
- **Sessions:** create, fork, revert, abort and summarize. `prompt_async` returns 204.
- **Events:** SSE at `/event`.
- **API:** OpenAPI 3.1 at `/doc`, and the SDK is generated from it.
- **Steps.** Per-agent `steps` caps the loop. On the last step tools are removed and the model is asked to summarize.
- **Subagents** are child sessions with fresh context that run in the background. The child uses its own permissions, not the parent's ([agents](https://opencode.ai/docs/agents/)).

### Tools
- **Built-in:** `read`, `glob`, `grep`, `shell`, `webfetch`, `websearch`, `question`, `skill` and `subagent`.
- **Edit tools:** `edit` (exact replace), `write`, and `patch` (GPT models only).
- **`execute`** is a Code Mode tool that runs JS and fronts MCP tools by default ([tools](https://opencode.ai/docs/tools/)).
- **Custom tools** are TS/JS files with Zod schemas ([custom-tools](https://opencode.ai/docs/custom-tools/)).
- **No sandbox.** The docs say `shell` "runs with the host user's filesystem, process, and network authority" ([permissions](https://opencode.ai/docs/permissions/)).

### Context
- **Compaction config:** `compaction.auto`, `keep.tokens` and `buffer` ([config](https://opencode.ai/docs/config/)).
- **Undo** is backed by filesystem snapshots.

### Permissions
- **Rules** are action + pattern mapped to `allow`, `deny` or `ask`. Last match wins.
- **Replies** are once, always or reject.
- **Defaults.** `external_directory` is allowed by default.
- **Directory inference** from command text is "best effort" ([permissions](https://opencode.ai/docs/permissions/)).

### Embedding
- **SDK.** `@opencode-ai/sdk` provides `createOpencode()`, which spawns the server, and `createOpencodeClient({baseUrl})` ([sdk](https://opencode.ai/docs/sdk/)).
- **Providers** come from the models.dev catalog, with 23 native packages plus OpenAI-compatible endpoints ([providers](https://opencode.ai/docs/providers/)). Whether the Vercel AI SDK sits underneath is UNVERIFIED.

### Builder pain
- **No in-process path.** Embedding always goes through the server.
- **Docs mix V1 and V2:** the server and SDK pages say V1, while the permissions and agents pages describe V2.
- **Sandboxing and budgets:**
  - Request for sandboxing ([#48411](https://github.com/anomalyco/opencode/issues/48411)).
  - Request for per-agent resource budgets ([#59006](https://github.com/anomalyco/opencode/issues/59006)).
  - No cap on parallel subagents ([#27110](https://github.com/anomalyco/opencode/issues/27110)).
- **Memory growth:**
  - Server RSS grows with session history ([#56058](https://github.com/anomalyco/opencode/issues/56058)).
  - Duplicate MCP servers reach about 6 GB ([#52965](https://github.com/anomalyco/opencode/issues/52965)).
- **Compaction bugs:** [#54910](https://github.com/anomalyco/opencode/issues/54910), [#47939](https://github.com/anomalyco/opencode/issues/47939).
- **Volume.** About 4,000 open issues.

## 5. pi (now `earendil-works/pi`, MIT, TypeScript)

The packages are `@earendil-works/pi-agent-core` and `@earendil-works/pi-ai` ([repo](https://github.com/earendil-works/pi)). How the old `@mariozechner/*` names and `badlogic/pi-mono` relate to these is UNVERIFIED.

### Loop
- **Agent.** `Agent` is stateful and emits typed events: `agent_start`, `turn_start`, `message_*`, `tool_execution_*`, `turn_end` and `agent_end` ([agent README](https://raw.githubusercontent.com/earendil-works/pi/main/packages/agent/README.md)).
- **Embedder hooks:**
  - `transformContext` and `convertToLlm` (required).
  - `prepareRequest`.
  - `beforeToolCall` and `afterToolCall`.
  - `finishTurn`, which returns end or continue.
- **Tool execution** is parallel by default.
- **Run control.** `steer()` and `followUp()` queues let the embedder steer a run. `agent_end` does not mean the run is over, so the SDK adds `agent_settled` ([sdk.md](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/sdk.md)).
- **No subagents, by design** ([coding-agent README](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/README.md)).

### Tools
- **Custom tools** go in `customTools`, and the active set can be filtered.
- **MCP** is built in over stdio and streamable HTTP. It is hidden behind "codemode" by default, results over 20 KB are truncated, and it is off in SDK sessions by default ([mcp.md](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/mcp.md)).
- **Built-in tool list:** UNVERIFIED.

### Context
- **Best-documented compaction in the survey.**
  - It triggers at the context window minus `reserveTokens` (16384) and keeps about 20k recent tokens.
  - Summaries follow a structured template (Goal, Constraints, Progress, Decisions, Next Steps).
  - Cuts never land on tool results.
  - Raw history is kept.
  - An extension hook can cancel or replace compaction ([compaction.md](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/compaction.md)).
- **Sessions** form a tree with branch summaries.

### Permissions
- **None.** Pi "does not ask for approval before every tool call". The docs recommend a container or VM ([security.md](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/security.md)).
- **Gating** is DIY through `beforeToolCall`.

### Providers
- **Coverage.** pi-ai has about 35 provider entries across 6+ wire protocols, and each provider owns its auth and streaming ([ai README](https://raw.githubusercontent.com/earendil-works/pi/main/packages/ai/README.md)).
- **Handoff limitation.** Cross-model handoff drops thinking blocks.

### Builder pain
- **Loop and compaction races:**
  - `waitForIdle()` resolves during compaction ([#10704](https://github.com/earendil-works/pi/issues/10704)).
  - A turn starts during compaction ([#10982](https://github.com/earendil-works/pi/issues/10982)).
  - A tool loop ends silently instead of compacting ([#12076](https://github.com/earendil-works/pi/issues/12076)).
- **SDK seams:**
  - MCP filtering before init ([#11048](https://github.com/earendil-works/pi/issues/11048)).
  - Customizing the identity preamble ([#11317](https://github.com/earendil-works/pi/issues/11317)).
  - Host-OS paths break remote file tools ([#5350](https://github.com/earendil-works/pi/issues/5350)).
- **Retries.** Mid-stream timeouts are not retried ([#12070](https://github.com/earendil-works/pi/issues/12070)).
- **Platform.** Node/TypeScript only.

## 6. Aider (Apache-2.0, Python)

### Loop
- **No tool-calling loop.** The model emits edit text, which Aider parses and applies.
- **Reflection.** `run_one` loops on reflections (lint or test failures), with `max_reflections = 3` ([base_coder.py](https://github.com/Aider-AI/aider/blob/main/aider/coders/base_coder.py)).
- **Output** is a stream of text, not typed events.
- **Edit formats:** `whole`, `diff`, `diff-fenced`, `udiff` and editor variants, chosen per model ([edit formats](https://aider.chat/docs/more/edit-formats.html)).

### Context
- **Repo map.** A graph-ranked repo map is the main idea worth copying. `--map-tokens` sets its budget, which defaults to 1k ([repomap](https://aider.chat/docs/repomap.html)).
- **History** is compressed by recursive summarization ([history.py](https://raw.githubusercontent.com/Aider-AI/aider/main/aider/history.py)).

### Embedding
- **Scripting API** is `Coder.create()` + `coder.run()`. The docs say it is "not officially supported or documented" and "could change in future releases without providing backwards compatibility" ([scripting](https://aider.chat/docs/scripting.html)).
- **Providers** go through litellm.
- **Dependencies** are heavy for a library: telemetry, audio and a TUI stack ([requirements.in](https://raw.githubusercontent.com/Aider-AI/aider/main/requirements/requirements.in)).

### Builder pain
- **No MCP.** The request has been open since February 2025 ([#3314](https://github.com/Aider-AI/aider/issues/3314)).
- **Missing pieces:** no subagents, no typed events, no sandbox.
- **Maintenance has stalled.** The latest release is v0.86.0 (2025-08-09), per the GitHub API.

## 7. Goose (now `aaif-goose/goose`, Apache-2.0, Rust)

Goose now sits under the Agentic AI Foundation at the Linux Foundation ([README](https://raw.githubusercontent.com/aaif-goose/goose/main/README.md)).

### Loop
- **Two implementations coexist.** The `goose-agent` crate unrolls the loop into ordered `Operation` steps over a persisted conversation ([goose-agent README](https://raw.githubusercontent.com/aaif-goose/goose/main/crates/goose-agent/README.md)).
- **Turn limit.** `GOOSE_MAX_TURNS` defaults to 1000.
- **Compaction** triggers at the lower of 80% of the window or 225k tokens ([context docs](https://goose-docs.ai/docs/guides/sessions/smart-context-management)).

### Tools
- **MCP-first,** with 70+ extensions (claimed).
- **Recipes** bundle prompts and extensions, with sub-recipes ([recipes](https://goose-docs.ai/docs/guides/recipes/)).

### Permissions
- **Modes:** autonomous, manual, smart and chat-only.
- **`ToolApprovalOperation`** returns Allow, Deny or Ask.

### Embedding
- **SDK.** `goose-sdk` is `0.1.0-alpha.13`, with UniFFI bindings for Python and Kotlin. It exposes a provider-level surface rather than an agent-loop surface ([sdk README](https://raw.githubusercontent.com/aaif-goose/goose/main/crates/goose-sdk/README.md)).
- **Other path:** ACP.

### Builder pain
- **No sandbox.** The macOS seatbelt sandbox was removed, and a Linux prototype is still a proposal ([#12805](https://github.com/aaif-goose/goose/issues/12805)).
- **ACP shell bypasses sandboxes.** In ACP, shell is delegated to the client terminal ([#12513](https://github.com/aaif-goose/goose/issues/12513)).
- **Subagents:**
  - They bypass the parent's approval mode ([#11778](https://github.com/aaif-goose/goose/issues/11778)).
  - Failures are reported as success ([#12988](https://github.com/aaif-goose/goose/issues/12988)).
  - They cannot be steered ([#12852](https://github.com/aaif-goose/goose/issues/12852)).
- **Extension model in flux.** A proposal would rewrite Operations as WASM plugins ([#12767](https://github.com/aaif-goose/goose/issues/12767)).

## 8. smolagents (Hugging Face, Apache-2.0, Python)

### Loop and executors
- **Code-as-action.** `CodeAgent` emits Python as its action. The docs argue this beats JSON tool calls ([secure execution](https://huggingface.co/docs/smolagents/en/tutorials/secure_code_execution)). The ToolCallingAgent description is UNVERIFIED.
- **Local executor.** The default `LocalPythonExecutor` is an AST interpreter with an import allowlist. The docs say "no local python sandbox can ever be completely secure".
- **Remote executors:** docker, e2b, modal and blaxel.
- **Multi-agent limitation.** Snippet sandboxing "doesn't support multi-agents".

### Builder pain
- **Security boundary:** a ctypes sandbox escape ([#2094](https://github.com/huggingface/smolagents/issues/2094)).
- **Interpreter fidelity:**
  - Decorators are ignored ([#2771](https://github.com/huggingface/smolagents/issues/2771)).
  - Class methods overwrite globals ([#3048](https://github.com/huggingface/smolagents/issues/3048)).
- **Requested features:**
  - Pre-tool-call authorization, the most-discussed request with 33 comments ([#2117](https://github.com/huggingface/smolagents/issues/2117)).
  - Cost caps and governance hooks ([#2654](https://github.com/huggingface/smolagents/issues/2654)).
  - A step timeout ([#1129](https://github.com/huggingface/smolagents/issues/1129)).
  - Async support ([#145](https://github.com/huggingface/smolagents/issues/145)).
  - Memory save and load ([#1216](https://github.com/huggingface/smolagents/issues/1216)).
- **MCP** has no resources or prompts ([#1378](https://github.com/huggingface/smolagents/issues/1378)).

## 9. Other harnesses that matter

### OpenHands software-agent-sdk (MIT, Python)
- **Surface.** Python SDK plus a REST and WebSocket agent server.
  - Workspaces: local, Docker or Kubernetes.
  - Tools: Terminal, FileEditor and TaskTracker.
  - Also: MCP, "automatic context compression", and an OpenAI-compatible endpoint ([repo](https://github.com/OpenHands/software-agent-sdk), [docs](https://docs.openhands.dev/sdk)).
- **Closest existing match** to an embeddable coding runtime with a sandbox.
- **Pain:**
  - One unknown event kind fails the whole conversation load ([#4080](https://github.com/OpenHands/software-agent-sdk/issues/4080)).
  - Confirmation policy is not persisted ([#4810](https://github.com/OpenHands/software-agent-sdk/issues/4810)).
  - Synchronous persistence blocks concurrency ([#5402](https://github.com/OpenHands/software-agent-sdk/issues/5402)).
  - Condenser redesign requests ([#5469](https://github.com/OpenHands/software-agent-sdk/issues/5469), [#6514](https://github.com/OpenHands/software-agent-sdk/issues/6514)).
- **Unverified:** condenser and confirmation internals.

### Vercel AI SDK (TypeScript)
- **Loop.** `ToolLoopAgent` stops at 20 steps by default.
  - `stopWhen` takes conditions.
  - `prepareStep` can rewrite the model, tools and messages per step, which is how compaction is done.
  - The loop stops when a call needs approval ([loop-control](https://ai-sdk.dev/docs/agents/loop-control)).
- **Pain.**
  - Approved calls run before the first step, so checkpoints miss them, and resume after a crash re-executes them ([#24624](https://github.com/vercel/ai/issues/24624)).
  - No typed denial ([#15842](https://github.com/vercel/ai/issues/15842)).
- **Assessment.** The best neutral TypeScript primitives. Persistence, sandbox and compaction are yours to build.

### Pydantic AI (Python)
- **Approvals.** Deferred tools handle human-in-the-loop.
- **Durable execution** comes from Temporal, DBOS or Prefect. `StepPersistence` covers simple cases, but "an engine is needed to recover a run that crashes mid-step".
- **OTel** spans are emitted ([docs](https://pydantic.dev/docs/ai/overview/)).
- **Compaction design is open** ([#11395](https://github.com/pydantic/pydantic-ai/issues/11395)).
- **Assessment.** Not coding-specific.

### LangGraph Deep Agents (MIT)
- **Features:**
  - Planning and a pluggable filesystem.
  - A sandboxed shell of your choice.
  - Subagents.
  - Summarization with tool-output offload.
  - MCP and human-in-the-loop.
- **Trust model.** "Trust the LLM": enforce boundaries in the sandbox ([README](https://github.com/langchain-ai/deepagents)).
- **Pain:**
  - Content-free compaction ([#6470](https://github.com/langchain-ai/deepagents/issues/6470)).
  - Images silently dropped on overflow ([#4954](https://github.com/langchain-ai/deepagents/issues/4954)).
  - Subagent tokens invisible ([#6556](https://github.com/langchain-ai/deepagents/issues/6556)).
- **Assessment.** Tied to the LangGraph runtime.

### Google ADK
- **Surface:**
  - Many languages.
  - Model adapters.
  - Function, MCP and OpenAPI tools.
  - Action confirmations.
  - Resume.
  - Context summarization ([adk.dev](https://adk.dev/)).
- **Assessment.** Not coding-specific, and oriented to Google Cloud.
- **Pain:** a task-mode agent loses its input after asking the user a question ([#7384](https://github.com/google/adk-python/issues/7384)).
- **Unverified:** license.

### Gemini CLI (Apache-2.0)
- **Surfaces:**
  - `packages/sdk` (`GeminiCliAgent.sendStream`).
  - Headless `stream-json` output.
  - An `experimental-acp` flag ([repo](https://github.com/google-gemini/gemini-cli)).
- **Gemini-only.** The README says free and Pro access moved to Antigravity CLI, so its future is uncertain.
- **Pain:** workspace policy files don't apply ([#18186](https://github.com/google-gemini/gemini-cli/issues/18186)).

### Mastra (TypeScript)
- **Category.** An app framework, not a coding loop.
- **Why it matters here:** evidence that suspend and resume are hard.
  - Stalls after a tool resume ([#28932](https://github.com/mastra-ai/mastra/issues/28932)).
  - A snapshot is written before every inference, about 300 ms ([#24628](https://github.com/mastra-ai/mastra/issues/24628)).

### Agent Client Protocol (ACP, Zed)
- **Transport.** JSON-RPC over stdio. Remote use over HTTP or WebSocket is "still a work in progress" ([intro](https://agentclientprotocol.com/overview/introduction)).
- **Methods:**
  - Session: `initialize`, `session/new`, `session/prompt` and optional `session/load`.
  - Agent to client: `session/request_permission` and `session/update`, which covers chunks, tool calls, plans and compactions.
  - Optional client capabilities: `fs/*` and `terminal/*` ([protocol](https://agentclientprotocol.com/protocol/overview)).
- **Adoption.** Goose, Gemini CLI and OpenCode expose it.
- **Scope.** It is an editor edge, not a library API.
- **Unverified:** stability status.

### Cline and Roo
- Not researched.
- They are IDE-extension agents rather than embeddable loops. That is my inference, UNVERIFIED.

---

## Cross-cutting gaps (input to the wedge decision)

1. **In-process, provider-neutral, coding-first: pick two.**
   - pi gets all three, but has no permissions, sandbox or subagents.
   - Every coding harness with real tooling and policy is a subprocess or server (Claude, Codex, OpenCode, OpenHands), vendor-tied (Claude, Codex, Gemini), or both.
2. **The process boundary is where embedders bleed.** Leaked children, frame corruption, pipe and stdin deadlocks, env leakage, and wire types drifting from the SDKs. The evidence is in the Claude and Codex SDK issues above.
3. **Sandboxing is outsourced.**
   - Only Codex ships OS confinement, and it degrades inside containers.
   - Nobody offers a neutral sandbox interface covering exec, fs, network policy and credential injection, with local and remote backends.
4. **Policy is thin, inconsistent, and doesn't propagate.**
   - Subagents bypass parent approvals (Goose #11778, OpenCode).
   - `allowed_tools` doesn't restrict (Claude).
   - Policy files fail to apply (Gemini).
   - Policy is not persisted (OpenHands #4810).
   - No typed denial (Vercel).
   - Pre-tool authorization is the most-requested smolagents feature.
5. **Approval × resume × crash is broken almost everywhere.** Approved calls re-execute (Vercel #24624). Pending approvals are lost (Agents SDK #5995). Approvals cross runs (Agents SDK #5413). Missing: idempotent tool-call IDs, an approval state that lives in a durable log, and exactly-once tool effects.
6. **Compaction is the top bug magnet, and nobody can test it.**
   - Content-free summaries (Deep Agents).
   - Races with turns (pi).
   - Provider-gated compaction (Codex).
   - Redesigns under way (Pydantic AI, OpenHands).
   - pi's structured, hookable compaction is the best reference.
7. **The event log is neither complete nor versioned.** Codex omits denied and code-mode exec. Claude's `usage` excludes subagents. OpenHands fails on one unknown event kind. Missing: an append-only, forward-compatible event log as the single source of truth.
8. **No deterministic replay or mock-model seam** (UNVERIFIED as absent). Builders cannot write a regression test for "the agent did X given model outputs Y".
9. **Budgets are coarse.** Step and token caps only. Subagent spend is invisible or overshoots (Claude, Deep Agents). There are no per-tenant or per-subagent wall-clock or dollar limits (Claude known limitations, OpenCode #59006).
10. **Multi-tenancy is opt-out.** Home-directory config and memory leak across tenants by default (Claude). Env and identity leak into exec (Codex).
11. **Observability has no shared schema.** Claude's OTel doesn't follow the GenAI conventions. Agents SDK tracing defaults to OpenAI. ACP is the only cross-vendor event vocabulary, and it covers only the editor edge.
12. **Churn and licensing.**
    - OpenCode is mid V1→V2.
    - Codex features sit behind experimental flags.
    - pi, OpenCode and Goose moved repos or renamed packages.
    - Claude Code is proprietary with branding limits.
    - Aider is stalled.

**Design references worth stealing:**
- pi: hooks and structured compaction.
- Codex: the OS sandbox, and the thread/turn/item protocol.
- Aider: the repo map.
- OpenHands: agent server and workspace split.
- Vercel AI SDK: `prepareStep` and `stopWhen`.
- Pydantic AI: durable-execution adapters.
- ACP: as an adapter surface.
