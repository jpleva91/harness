# Glossary

- **Harness**: the runtime that runs one agent's loop: model calls, tool execution, context and permissions. It is a library the agent builder imports, not a process they supervise.
- **Agent builder**: the person or team who embeds the harness in their own product or pipeline. The first builder v1 serves is a product team shipping a coding agent inside a multi-tenant product or backend.
- **Wedge**: the one primary reason an agent builder picks this harness, used to settle scope arguments. The wedge is *safe in-process* ([#5](https://github.com/jpleva91/harness/issues/5)).
- **In-process**: the loop, model calls, context and permission decisions run inside the builder's process. Tool execution may run in sandboxed child processes that the harness manages.
- **Ambient state**: anything the harness would read without being handed it: home directory, global config, environment variables, memory files. The harness reads none.
- **Safe**: deny unless allowed, sandbox on, network off, until the builder loosens it. The harness contains a misbehaving or prompt-injected agent and keeps tenants' state apart. It is not a boundary against hostile tenant code.
- **Durable approvals**: approvals and sessions that survive a crash or resume. First claim on v1 scope after the wedge.
