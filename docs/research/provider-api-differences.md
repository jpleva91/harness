# Provider API differences for a neutral model interface

Research note for issue #3. Read on 2026-11-14.

**Question.** What concretely differs between the Anthropic Messages API, the OpenAI Responses API and local / OpenAI-compatible servers (Ollama, llama.cpp `llama-server`, vLLM) in tool calling, streaming, reasoning, prompt caching, structured output and error semantics? What must a provider-neutral interface cover, and what does it lose?

**Short answer.** On the request side, a neutral interface is easy: messages, tools as JSON Schema, a stream of text and tool-call events. The hard parts are elsewhere. Every provider attaches opaque state to the history and requires it back on the next turn (thinking signatures, encrypted reasoning, template-specific reasoning fields). Providers disagree about what happens at the edges: context overflow, forced tool use, refusals, overload. Caching and server-side tools are economic and behavioural features, not wire formats. The interface should be a stateless, append-only replay of provider-owned items, with the edges exposed as explicit capabilities rather than hidden.

## Provenance key

Every claim carries one of these tags:

- **[F]**: I fetched the page live this session (platform.claude.com rate-limits and streaming pages).
- **[A]**: Anthropic's bundled `claude-api` skill reference. It is a first-party snapshot cached 2026-10-06, not re-fetched page by page. The equivalent live pages are listed under Sources.
- **[S]**: A research subagent read the primary source (official docs, or SDK/server source on the `main`/`master` branch, not pinned to a commit) and gave the URL. I did not open these pages myself.
- **UNVERIFIED**: Neither I nor a subagent could confirm the claim from a primary source. These tags are carried over verbatim from the subagent reports.

Doc host note: `platform.openai.com/docs/*` now redirects to `developers.openai.com/api/docs/*` [S].

## Difference table

"OA" means the server's OpenAI Chat Completions-compatible endpoint. All three local servers also expose `/v1/responses` and an Anthropic-style `/v1/messages`, each with partial coverage (see "Surfaces" below).

| Concern | Anthropic Messages | OpenAI Responses | Ollama | llama.cpp server | vLLM |
|---|---|---|---|---|---|
| **Tool definition** | `{name, description, input_schema, strict?}`; Anthropic-defined schema-less tools (`bash_20250124`, text editor) [A] | `{type:"function", name, description, parameters, strict}`; strict is attempted by default (falls back to `strict:false` when the schema can't be made strict) [S] | OA `tools`; native `/api/chat` `tools` [S] | OA `tools`; needs Jinja templates (`--jinja`, now on by default) [S] | OA `tools`; needs `--enable-auto-tool-choice --tool-call-parser <name>` [S] |
| **How tool calls are implemented** | Native model output | Native model output | Per-model renderer/parser in the server [S] | Jinja chat template, then a per-family parser or a "Generic" fallback [S] | Chat template plus a selectable parser (hermes, llama3_json, mistral, hf, …) [S] |
| **Call shape and ID** | `tool_use` content block with server-issued `id` and `input` **as an object** [A][F] | `function_call` output item with `call_id`, `arguments` **as a JSON string** [S] | Native: `arguments` is an **object**, results matched by `tool_name`. OA: converted to a JSON string, with an 8-character random ID generated server-side [S] | `arguments` is a JSON string (not traced exhaustively); IDs generated when missing [S] | `arguments` is a JSON string [S]; ID format UNVERIFIED |
| **Result shape** | `tool_result` block in a `user` message, keyed by `tool_use_id`, with `is_error: true` for failures; all parallel results go in **one** user message [A] | `function_call_output` input item `{call_id, output}`; `output` is typically a string; no `is_error` flag [S] | Native `tool_name`, OA `tool_call_id` [S] | OA `tool_call_id` | OA `tool_call_id` |
| **`tool_choice`** | `auto`, `none`, `any`, `{type:"tool"}`. **`any` and `tool` return 400 on Fable 5.1, Mythos 5.1, Opus 5.5 and Sonnet 5.5** [A] | `auto`, `none`, `required`, `{type:"function",name}`, `{type:"allowed_tools", mode, tools}` [S] | **Not supported** (OA and Anthropic-compat) [S] | OA accepts only the strings `auto`, `none` and `required`; anything else throws. Named function UNVERIFIED, probably unsupported [S] | `auto`, `none`, `required`, named. Named and required go through the grammar backend, so the first request pays a compile cost [S] |
| **Parallel calls** | On by default; `disable_parallel_tool_use` [A] | `parallel_tool_calls` (`false` gives zero or one call); built-in tools can't be in a parallel batch on GPT-5+ [S] | Supported (calls carry an `index`) [S] | **Off by default**; `parallel_tool_calls:true`, and only some templates support it [S] | On by default [S] |
| **Streaming transport** | SSE with named events: `message_start`, `content_block_start`/`_delta`/`_stop`, `message_delta`, `message_stop`, `ping`, `error`. Clients must ignore unknown event types [F] | SSE with semantic events (`response.created`, `response.output_item.added`, `response.output_text.delta`, `response.function_call_arguments.delta`/`.done`, `response.completed`, `response.failed`, `response.incomplete`, `error`); each event has a `sequence_number`; background streams can resume with `starting_after` [S] | Native: **NDJSON**. OA: SSE ending in `data: [DONE]` [S] | SSE [S] | SSE [S] |
| **Tool-argument streaming** | `input_json_delta` partial JSON. **Buffered per key by default**; `eager_input_streaming: true` streams unvalidated fragments [F][A] | `function_call_arguments.delta`, then `.done` with the full arguments [S] | **Whole call in a single chunk**, no argument deltas [S] | Per-call argument deltas, with id and name on the first delta [S] | Parser-dependent: `hf` sends whole calls, others stream deltas (not checked per parser) [S] |
| **Usage in stream** | Cumulative `usage` on `message_delta` [F] | On `response.completed` (from the guide) [S] | `stream_options.include_usage` [S] | `include_usage` handled [S] | `stream_options.include_usage`, default false [S] |
| **Mid-stream errors** | `event: error` with e.g. `overloaded_error` after a 200 response has already started [F] | `error` event (`code`, `message`, `param`) and `response.failed` [S] | Native: an `{"error":…}` NDJSON line, HTTP status unchanged. Anthropic-compat: streamed errors listed as unsupported [S] | UNVERIFIED | UNVERIFIED |
| **Reasoning control** | `thinking: {type:"adaptive"}` plus `output_config.effort` (`low`…`max`). Rules vary by model: Opus 5.5 can't disable thinking (400); Sonnet 5.5 uses `between_tools` to turn it off; older models take `budget_tokens` [A] | `reasoning: {effort, summary, context, mode}`; effort runs `none`…`max` with model-specific support (e.g. gpt-6.1-sol rejects `none`/`minimal`) [S] | `think`: bool, level or null. OA: `reasoning_effort` [S] | `--reasoning-format`, per-request `reasoning_format`, `reasoning_effort`, `--reasoning-budget`, `chat_template_kwargs` [S] | `--reasoning-parser`, `include_reasoning`, `thinking_token_budget`, `chat_template_kwargs`, effort mapping [S] |
| **Reasoning output** | `thinking` content block with a `signature` (`thinking_delta` then `signature_delta`); `redacted_thinking` arrives whole. Display defaults to `"omitted"` (empty text) on current models; the raw chain of thought is never returned [F][A] | `reasoning` output items, opaque. Summaries only on opt-in; `encrypted_content` for stateless use. Raw chain of thought not exposed, although a `response.reasoning_text.delta` event exists (when it fires is UNVERIFIED) [S] | Native `message.thinking`; OA `reasoning` [S] | `message.reasoning_content` [S] | **`reasoning`** (renamed from `reasoning_content`; old clients silently read empty) [S] |
| **Reasoning passback** | Pass thinking blocks back unchanged. Blocks are tied to the producing model, and on new models to the conversation ("preserved thinking": edited history invalidates or 400s on enforced accounts). Other models drop them silently [A] | Pass reasoning items back with the tool outputs. Stateless or ZDR mode needs `include:["reasoning.encrypted_content"]` (sources conflict on whether this is still required, so send it) [S] | Assistant `thinking` (native) or `reasoning` (OA) accepted [S] | Only "by some specific templates" [S] | Shown for `kimi_k2`/`minimax_m2`; general re-rendering UNVERIFIED [S] |
| **Prompt caching mechanism** | Explicit `cache_control` breakpoints, max 4, or top-level auto-placement. Prefix match in the order `tools`, `system`, `messages`. TTL 5m (default) or 1h. Model-dependent minimum of 512–4096 tokens; below it caching fails silently [A] | Implicit by default. **GPT-5.6+ adds explicit `prompt_cache_breakpoint` (up to 4 writes)**, `prompt_cache_options{mode, ttl: 30m or 12h, prewarm}`, `prompt_cache_key` routing hint. 1,024-token minimum on 5.6+. `prompt_cache_retention` deprecated [S] | Implicit KV reuse while the model stays loaded (`keep_alive` 5m). Anthropic-compat ignores `cache_control` [S] | `cache_prompt` (default true) reuses the slot KV prefix and may make results nondeterministic; `--cache-reuse`, `--cache-ram`, slot save/restore [S] | Automatic prefix caching, on by default [S] |
| **Cache accounting** | `cache_creation_input_tokens`, `cache_read_input_tokens`; `input_tokens` counts only tokens after the last breakpoint. Cached reads don't count toward ITPM [F] | `input_tokens_details.cached_tokens`, `cache_write_tokens` (+ per-TTL details), `prompt_cache_diagnostics` [S] | Native `prompt_eval_cached_count`; OA `prompt_tokens_details.cached_tokens` [S] | `prompt_tokens_details.cached_tokens`, `timings.cache_n` [S] | `prompt_tokens_details.cached_tokens` only with `--enable-prompt-tokens-details` (whether that defaults off is UNVERIFIED) [S] |
| **Cache pricing** | Writes 1.25x (5m) or 2x (1h); reads about 0.1x base (0.05x Opus 5.5, 0.025x Fable 5.1) [A] | GPT-5.6+: writes 1.25x (30m) or 2.5x (12h); reads 0.1x (0.05x on gpt-6.1 sol/terra) [S] | Free (local compute) | Free | Free |
| **Structured output** | `output_config.format` (JSON schema) plus `strict: true` on tools. Supports `anyOf`, `allOf`, `$ref`, some formats. **No recursion, no numeric or string-length constraints**; `additionalProperties:false` required. Incompatible with citations and prefill. Schemas compile once, then are cached 24h [A] | `text.format{type:"json_schema", strict}`. Every field required (use null unions for optional), `additionalProperties:false`. Supports recursion, `pattern`, min/max. **No `allOf`, `not`, `if`/`then`**. Size limits (5,000 properties, 10 nesting levels, …) [S] | Native `format` (`"json"` or schema); OA `response_format` (strictness UNVERIFIED) [S] | `response_format` (json_object / json_schema), plus GBNF `grammar` and `json_schema` on `/completion` [S] | `response_format` json_schema or `structured_outputs{json, regex, choice, grammar, structural_tag}`; backends xgrammar / guidance; `guided_*` removed in v0.12 [S] |
| **Error body** | `{"type":"error","error":{"type","message","details?"},"request_id"}` [A][F] | `{"error":{message, type, param, code}}` [S] | Native `{"error":"<string>"}`; OA OpenAI-shaped [S] | OpenAI-shaped, plus types such as `exceed_context_size_error`, `not_supported_error` (501), `unavailable_error` (503) [S] | OpenAI-shaped [S] |
| **Error codes** | 400, 401, 402 billing, 403, 404, 413, 429, 500, **529 overloaded** [A] | 400, 401, 403, 404, 409, 422, 429, ≥500 (503 `server_is_overloaded`) [S] | 400, 404, 429, 500, 502; 503 when the queue is full (`OLLAMA_MAX_QUEUE`) [S] | 503 while loading; behaviour when all slots are busy UNVERIFIED [S] | 400 on validation; 503 on overload UNVERIFIED [S] |
| **Rate-limit signalling** | `retry-after` plus `anthropic-ratelimit-{requests,tokens,input-tokens,output-tokens}-{limit,remaining,reset}` (RFC 3339 reset). A 429 can be a rate limit, a ramp limit (`error.details.error_code: ramp_limit_exceeded`) or the spend cap (`enforced_spend_limit_reached`, no `retry-after`, retrying is useless) [F] | `x-ratelimit-{limit,remaining,reset}-{requests,tokens}`, `retry-after(-ms)`; `insufficient_quota` must not be retried [S] | None documented [S] | None documented [S] | None documented [S] |
| **Context overflow** | 400 on input. On output, stop reason `model_context_window_exceeded` (4.5+) [A] | `truncation: "disabled"` by default, so a 400 [S] | **Silent truncation or shift by default** (`truncate`/`shift` default true) [S] | `--context-shift` off by default; inferred to return 400 `exceed_context_size_error` (partially verified) [S] | "maximum context length" error unless `truncate_prompt_tokens` is set; status probably 400 (UNVERIFIED) [S] |
| **Context size** | Fixed per model (1M on current models) [A] | Per model | Configured: `num_ctx` / `OLLAMA_CONTEXT_LENGTH`; **the default is in conflict** (FAQ says 4096; context-length doc says VRAM-tiered 4k/32k/256k); OA has no per-request override [S] | `-c`, default from the model [S] | `--max-model-len`, default from model config [S] |
| **Termination vocabulary** | `stop_reason`: `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`, `pause_turn`, `refusal` (+ `stop_details.category`), `model_context_window_exceeded` [A] | `status` `completed`/`incomplete`/`failed`/`cancelled`/`queued`/`in_progress`; `incomplete_details.reason` `max_output_tokens`/`max_messages`/`content_filter`/`steered`; refusals arrive as a `refusal` content part [S] | OA `finish_reason` (OpenAI Chat vocabulary; not separately verified) | Same | Same |
| **Server state** | Stateless; client replays history. Server-side compaction is beta and returns compaction blocks that must be replayed [A] | `store` defaults to true (30 days). `previous_response_id`, Conversations API, server compaction, background mode with poll/cancel [S] | Stateless (`/v1/responses` without `previous_response_id`) [S] | Stateless | `/v1/responses` with `/{id}` and cancel [S] |
| **Server-side tools** | web search/fetch, code execution, tool search, MCP connector; `pause_turn` after 10 server iterations; server-tool errors come back as 200 with an error block [A] | web_search, file_search, code_interpreter, image_generation, computer, shell, apply_patch, remote MCP, tool_search [S] | None | None | Anthropic-compat returns 400 for server tools [S] |
| **Other request-shape traps** | **No assistant prefill on 4.6+ models (400)**; `temperature`/`top_p`/`top_k` 400 on newer models and on every model from API version 2026-10-21; roles must alternate; mid-conversation `system` messages on newer models [A] | `instructions` not carried across `previous_response_id` [S] | — | — | — |

### Surfaces on the local servers

All three local servers now serve OpenAI Chat Completions, OpenAI Responses and Anthropic Messages, plus native APIs [S]. Coverage is partial on each:

- **Ollama.** `/v1/responses` is stateless only. `/v1/messages` lacks `tool_choice` and `cache_control`, and accepts `budget_tokens` without enforcing it.
- **vLLM.** `/v1/messages` rejects `max_tokens=0` and server tools.

A wire-compatible endpoint therefore doesn't make a local model behave like the hosted API it imitates. The harness should talk to local servers through an OA (Chat Completions) adapter with capability flags. It should not reuse the Anthropic or OpenAI adapter against a local URL.

## What a neutral interface must cover

1. **History of typed items, not strings.** Text, tool call, tool result (with an error flag), and **opaque provider items**: Anthropic thinking and redacted-thinking blocks with signatures, OpenAI reasoning items with `encrypted_content`, compaction blocks, local `reasoning`/`thinking` text. These pass through unchanged and are tagged with the provider and model that produced them.
2. **Append-only conversation semantics.** Anthropic's preserved thinking punishes edited history, and OpenAI caching is prefix-based. The harness has to treat history as append-only and model compaction as an explicit, provider-specific operation.
3. **Tool-call events that tolerate every arrival pattern.** Argument deltas (Anthropic, OpenAI, llama.cpp, most vLLM parsers), whole calls (Ollama, vLLM `hf`) and buffered-per-key (Anthropic default). The interface should emit `tool_call_start` / `args_delta?` / `tool_call_end` with complete arguments, where deltas are optional. Normalise arguments to a parsed object and validate them client-side against the schema, because eager streaming and local parsers give no guarantee.
4. **Harness-owned call IDs.** Map provider IDs (`id`, `call_id`, generated 8-character, missing) to stable harness IDs, and route results back in each provider's shape. One special case: Anthropic expects all parallel results in one user message.
5. **A stop-reason superset with the raw value attached.** `end_turn | tool_calls | max_output | context_overflow | refusal(category) | paused(server_tool) | content_filter | cancelled | error`, plus `provider_detail`.
6. **Usage with cache fields.** `input_uncached`, `cache_read`, `cache_write` (by TTL), `output`, `reasoning_output`. Each field is optional, because vLLM omits details without a flag.
7. **An error taxonomy, not status codes.** Classify errors as `rate_limited(retry_after?)`, `overloaded` (529 or 503), `quota_exhausted` (non-retryable: Anthropic spend cap, OpenAI `insufficient_quota`), `context_overflow`, `invalid_request`, `auth`, `server`, `mid_stream_error`. The raw body stays attached.
8. **A capabilities object per provider and model.** It records `forced_tool_choice`, `parallel_tools` (with default), `tool_arg_streaming`, `reasoning_passback`, `explicit_cache_breakpoints`, `structured_output` (schema dialect), `prefill`, `sampling_params`, `server_tools`, `context_overflow_behavior`, and `context_window` (configured, for local servers).
9. **Cache hints as advisory metadata.** "Stable prefix ends here" markers that the Anthropic adapter (and the OpenAI adapter on GPT-5.6+) turns into breakpoints, and that other adapters drop.

## What a neutral layer cannot paper over

These are the losses. Each needs a capability flag or harness-side mitigation; a type alone doesn't solve it.

1. **Reasoning continuity.** Anthropic thinking blocks are bound to the producing model and, on new models, to the unedited conversation. OpenAI reasoning is encrypted and only works on OpenAI. Local reasoning passback works only for some templates or parsers. Switching providers or models mid-conversation **loses the reasoning state**; the most the harness can do is drop it cleanly. There is no portable "thought".
2. **Context overflow.** Ollama truncates or shifts silently by default, and its default window is small and disputed (4096 vs VRAM-tiered). The others return 400. The harness has to **count tokens itself** and enforce its own budget, or a local run can silently forget the system prompt or early tool results. Mitigation: always set `num_ctx` explicitly, and treat a provider-reported `prompt_tokens` that is smaller than expected as overflow.
3. **Forced tool use.** It can't be relied on across providers. Anthropic's newest models return 400 on `any`/`tool`, Ollama ignores `tool_choice`, and llama.cpp accepts only strings. "Make the model call tool X" has to become `auto` plus a prompt instruction plus a check-and-retry loop. Use structured output when the goal was really JSON.
4. **Structured-output schema dialects.** Anthropic has no recursion or numeric/length constraints but allows `allOf`. OpenAI requires every field and disallows `allOf`/`not`. Local servers use grammar backends whose coverage varies. A portable schema is the intersection, or the full schema validated client-side with a retry. Schema guarantees are per provider.
5. **Caching economics and control.** Anthropic needs explicit breakpoints, with a minimum prefix under which caching fails silently, and charges for writes. OpenAI is implicit, with optional breakpoints on 5.6+ and different TTL choices and prices. Local caching is free but per-slot and can introduce nondeterminism (llama.cpp). The interface can carry hints, but **cost and hit rate stay provider-specific**, and a layout that is optimal for one isn't optimal for another.
6. **Streaming fidelity.** Ollama has no argument deltas, and Anthropic buffers per key unless eager streaming is on. A UI that shows a tool call "as it is typed" works on some providers only. Mid-stream errors after HTTP 200 exist on Anthropic, OpenAI and Ollama native, so the harness's retry logic must handle failure after partial output: discard and replay the request, never resume it.
7. **Server-side tools and server state.** Hosted web search, code execution, shell and MCP, along with `pause_turn`, OpenAI `previous_response_id`, Conversations, background mode and server compaction, have no local equivalent. A neutral v1 should be **stateless replay with client-executed tools only**. Hosted tools would be an optional extension per provider, outside the core interface.
8. **Refusals and safety signalling.** Anthropic returns HTTP 200 with `stop_reason: "refusal"` and a category, can cut off a `tool_use` mid-input, and offers server-side model fallback. OpenAI uses a `refusal` content part or `content_filter`. Local models have no signal at all. The harness can normalise "refused", but not why, and it can't detect a local model that silently declines.
9. **Rate-limit and overload semantics.** Anthropic distinguishes rate, ramp and spend-cap 429s through `error.details.error_code`, and uses 529 for overload. OpenAI uses different headers, and its quota errors must not be retried. Local servers send no headers, and queueing behaviour varies (Ollama 503 on a full queue; llama.cpp and vLLM UNVERIFIED). Backoff policy must be per provider. A generic "retry 429" policy loops forever on Anthropic's spend cap.
10. **Request-shape restrictions that move per model.** Prefill is gone on Anthropic 4.6+, sampling parameters return 400 on newer Anthropic models, thinking can't be disabled on Opus 5.5, and effort levels vary by OpenAI model. These change with each model release, so the capabilities object has to be data, updated per model, not code.

## Corrections to assumptions going in

- **OpenAI caching.** I expected purely implicit caching with `prompt_cache_retention`. On GPT-5.6+, OpenAI has explicit breakpoints, `prompt_cache_options.ttl`, and charges for cache writes, so on that row it is now structurally close to Anthropic. `prompt_cache_retention` is deprecated [S].
- **llama.cpp `--jinja`.** It is now enabled by default. The function-calling doc still says to pass it [S].
- **vLLM reasoning field.** It is `reasoning`, not `reasoning_content` [S].
- **Local servers' API surfaces.** All three now expose Responses- and Messages-compatible endpoints [S].
- **Open conflict.** Ollama's default context window: the FAQ says 4096, while `docs/context-length.mdx` says it is VRAM-tiered. Not resolved here.

## Unverified items (consolidated)

- OpenAI: when `response.reasoning_text.delta` fires, given that raw chain of thought isn't exposed. Whether `include: ["reasoning.encrypted_content"]` is still required in stateless mode. Where `apply_patch` and `computer` execute. Whether the SDK retries timeouts and connection errors (inferred from log lines). The full streaming-events reference, which was too large to fetch.
- Ollama: OA `response_format` json_schema strictness.
- llama.cpp: named-function `tool_choice` on `/v1/chat/completions`. Exact context-overflow error path. Behaviour when all slots are busy. Mid-stream error format.
- vLLM: tool-call ID format. Status code for context overflow. 503 on overload. Whether `--enable-prompt-tokens-details` defaults off. Reasoning passback outside `kimi_k2`/`minimax_m2`. Mid-stream error format.
- Local `finish_reason` vocabularies were assumed to follow OpenAI Chat Completions; not checked.

## Sources

Anthropic (live pages behind [A]; [F] pages fetched this session):
- [F] https://platform.claude.com/docs/en/api/rate-limits.md
- [F] https://platform.claude.com/docs/en/build-with-claude/streaming.md
- [A] https://platform.claude.com/docs/en/api/errors.md
- [A] https://platform.claude.com/docs/en/build-with-claude/prompt-caching.md
- [A] https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview.md
- [A] https://platform.claude.com/docs/en/build-with-claude/adaptive-thinking.md, https://platform.claude.com/docs/en/build-with-claude/effort.md
- [A] https://platform.claude.com/docs/en/about-claude/models/migration-guide.md (prefill, forced tool choice, preserved thinking, sampling parameters, `model_context_window_exceeded`)

OpenAI [S]:
- https://developers.openai.com/api/docs/guides/function-calling
- https://developers.openai.com/api/docs/guides/tools
- https://developers.openai.com/api/docs/guides/conversation-state
- https://developers.openai.com/api/docs/guides/streaming-responses
- https://developers.openai.com/api/docs/guides/reasoning
- https://developers.openai.com/api/docs/guides/prompt-caching
- https://developers.openai.com/api/docs/guides/structured-outputs
- https://developers.openai.com/api/docs/guides/error-codes
- https://developers.openai.com/api/docs/guides/rate-limits
- https://developers.openai.com/api/docs/guides/background
- https://developers.openai.com/api/docs/guides/migrate-to-responses
- https://developers.openai.com/api/docs/deprecations
- openai-python `main` (v3.51.0): `src/openai/types/responses/{response_create_params,tool,response_stream_event,response_usage,response_status,response}.py`, `types/shared/reasoning.py`, `_client.py`, `_constants.py`, `_base_client.py`

Ollama [S]:
- github.com/ollama/ollama `docs/api.md`
- github.com/ollama/ollama `docs/api/openai-compatibility.mdx`
- github.com/ollama/ollama `docs/api/anthropic-compatibility.mdx`
- github.com/ollama/ollama `docs/api/errors.mdx`
- github.com/ollama/ollama `docs/capabilities/tool-calling.mdx`
- github.com/ollama/ollama `docs/capabilities/thinking.mdx`
- github.com/ollama/ollama `docs/faq.mdx`
- github.com/ollama/ollama `docs/context-length.mdx`
- Source files: `server/routes.go`, `openai/openai.go`, `middleware/openai.go`, `api/types.go`

llama.cpp [S]:
- github.com/ggml-org/llama.cpp `tools/server/README.md`
- github.com/ggml-org/llama.cpp `docs/function-calling.md`
- Source files: `tools/server/server-common.cpp`, `tools/server/server-task.cpp`, `common/chat.cpp`

vLLM [S] (github.com/vllm-project/vllm `docs/`, rendered at docs.vllm.ai):
- `serving/online_serving/openai_compatible_server.md`
- `serving/online_serving/README.md`
- `serving/integrations/claude_code.md`
- `features/tool_calling.md`
- `features/reasoning_outputs.md`
- `features/interleaved_thinking.md`
- `features/automatic_prefix_caching.md`
- `features/structured_outputs.md`
- Source files: `vllm/config/cache.py`, `vllm/config/model.py`, `vllm/entrypoints/generate/base/protocol.py`, `vllm/entrypoints/serve/engine/protocol.py`, `vllm/renderers/params.py`
