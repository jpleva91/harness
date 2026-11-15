---
name: formal-verify-loop
description: "Check the loop prototype's cancel and approval rules with a TLA+ model and replay tests. Use after any change to prototype/loop-ffi/core/src/lib.rs (run, hosted, start, the Host trait), or when a change touches when approvals are asked, when tools start, or what cancel does."
---

# Formally verify the loop's cancel and approval rules

The loop in `prototype/loop-ffi/core/src/lib.rs` calls the builder's approval handler and tools while the builder may cancel at any moment. Tests reach only the orderings someone wrote down. This check covers every ordering of one bounded run.

## The rules

| Rule | In plain words | Harm if it breaks |
| --- | --- | --- |
| `NoHostCallStartsAfterCancel` | Once the builder has cancelled, the loop starts no new approval prompt and no new tool. | A user presses stop and a destructive tool still starts, or a reviewer is asked to approve work for a run that is over. |
| `ToolRunsOnlyWhenAllowed` | A tool runs only if its approval handler answered allow. A handler that fails counts as a denial. | A crashed policy service lets every tool through. |
| `NoOrphanHostCall` | When the run is over, no host callback is left running without an abort. | A cancelled run leaves a tool working in the builder's process with nobody waiting for it. |

## Run it

```sh
.claude/skills/formal-verify-loop/check.sh
```

It takes about ten seconds and needs Java 11 or later (`JAVA=/path/to/java` if `java` on `PATH` is older) and `cargo`. It fetches TLC 1.7.4 once, pinned by checksum, to `~/.cache/tlaplus/`. It runs three things and prints `ALL CHECKS PASS` or `CHECKS FAILED`:

1. **Model.** TLC checks `model/Loop.tla` with `model/Loop.cfg`. Every rule must hold.
2. **Mutants.** Each rule is checked against a broken copy of the model and must fail there. A mutant that passes means the rule can no longer fail, so the check proves nothing.
3. **Code.** `cargo test -p proto-core --test loop_rules` replays each rule's bug path against the real loop.

## What is covered

| Model action | Code (`prototype/loop-ffi/core/src/lib.rs`) |
| --- | --- |
| `Send`, `StopAtEmit` | the `emit` closure in `run`: a select over cancel and `tx.send` |
| `AskApproval` | `emit(ToolCall)`, `emit(ApprovalAsked)`, then `host.approve(..)` in the same poll |
| `TakeAnswer`, `AbandonApproval` | `hosted(..)` around the approval future |
| `AnswerAllowed`, `AnswerDenied`, `ReportDenied` | `emit(ApprovalAnswered)`, then `host.call_tool(..)` or the denied `ToolResult` |
| `TakeResult`, `ToolTimesOut`, `AbandonTool` | `hosted(..)` around the tool future, with its timeout |
| `ReportResult`, `SendFinished` | `emit(ToolResult)`; `tx.send(RunFinished)` in `start` |
| `CancelFirst = TRUE` | `biased;` with the cancel branch first in every `select!`. The select in `emit` is the one that guards `NoHostCallStartsAfterCancel`, because an emit comes before every host call. Unbiasing only `hosted` does not break the rule. A host call started without an emit before it would. |
| `BuilderCancels`, `BuilderReadsEvent`, `HostAnswers`, `ToolSettles` | `Handle::cancel`, the receiver, and the `Host` futures settling |

One model step is one poll of the run task: the code between two awaits.

Replay tests: `prototype/loop-ffi/core/tests/loop_rules.rs`, one test per rule, named in each doc comment.

**Sync commit: `d56f096`.** The model described the code at that commit. If `git log d56f096.. -- prototype/loop-ffi/core/src/lib.rs` shows changes, read them against the table before trusting a pass. Move the sync commit forward only after you have confirmed the table still describes the code.

## Bounds and what is left out

- One run, two tool calls, one after another, event channel capacity two. Parallel tool calls are not modeled; the loop has none.
- The model checks safety only. It does not show that a run ends. A builder who stops reading events without cancelling parks the loop for as long as the session object lives.
- Cancel is another thread. A cancel that lands in the middle of a poll is seen by the next poll, so one host call can still start and is then aborted at once. The rule is about polls that begin after cancel.
- The FFI layers (`node/`, `py/`) are not modeled. Whether an abort reaches the host callback is covered by the demos, not here.
- Text deltas, retries, steering and turns are left out: they start no host call.
- The replay tests for `NoHostCallStartsAfterCancel` are statistical: 2000 runs each. The broken loop failed 126 and 702 of 2000.

## When a check fails

- **The model fails.** Read the result with `/pstack:statechart` on `model/Loop.tla`, `model/Loop.cfg` and `model/view.ts`. It gives the shortest path to the broken rule in plain words. Replay that path as a test in `loop_rules.rs`. If the real loop does not do it, the model is wrong; fix the model. If it does, it is a bug: report it with the path, and do not weaken the rule.
- **A mutant fails.** `the mutation matched nothing` means the model text changed and the `sed` in `check.sh` needs the new text. `wanted X to break` means the rule no longer guards what it claims.
- **A replay test fails and the model passes.** The code left the model. Find which row of the table no longer holds.

## When a change is intended

Change the model in the same commit as the code: add the action or variable, map it in the table, add a mutant if you add a rule, add a replay test for the rule, run the check, then move the sync commit.

When the check passes, `/pstack:statechart` also draws how a run moves between states, which is the quickest way to review a model change.
