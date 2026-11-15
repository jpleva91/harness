---- MODULE Loop ----
\* Model of one run of the loop in prototype/loop-ffi/core/src/lib.rs.
\* Each loop action is the code between two awaits. One poll of the run task may take
\* several actions in a row when their awaits are ready; that only adds orderings.
EXTENDS Naturals

CONSTANTS
  Calls,       \* tool calls the model asks for in the run
  Capacity,    \* event channel capacity (Config.event_capacity)
  CancelFirst  \* TRUE: every select looks at cancel before any other ready branch (biased)

VARIABLES
  pc,         \* where run() is waiting
  call,       \* index of the tool call being worked
  cancelled,  \* Handle.cancel() was called, or the builder dropped the session
  approval,   \* the host's approve callback for this call
  allowed,    \* the decision run() took from it
  tool,       \* the host's call_tool callback for this call
  queued,     \* events in the channel, not yet read by the builder
  lateStart   \* a host callback was started by a poll that began after cancel

vars == <<pc, call, cancelled, approval, allowed, tool, queued, lateStart>>

Emits == {"emitCall", "emitAsked", "emitAnswered", "emitDenied", "emitResult"}

Init ==
  /\ pc = "emitCall" /\ call = 1 /\ cancelled = FALSE
  /\ approval = "none" /\ allowed = FALSE /\ tool = "none"
  /\ queued = 0 /\ lateStart = FALSE

\* A select may take a non-cancel branch.
MayProceed == CancelFirst => ~cancelled

\* emit(): tx.send wins the select.
Send == MayProceed /\ queued < Capacity /\ queued' = queued + 1

NextCall ==
  IF call < Calls
    THEN /\ call' = call + 1 /\ pc' = "emitCall"
         /\ approval' = "none" /\ tool' = "none" /\ allowed' = FALSE
    ELSE /\ pc' = "finish" /\ UNCHANGED <<call, approval, tool, allowed>>

\* ---- the builder and the host ----

BuilderCancels == ~cancelled /\ pc # "done" /\ cancelled' = TRUE
  /\ UNCHANGED <<pc, call, approval, allowed, tool, queued, lateStart>>

BuilderReadsEvent == queued > 0 /\ queued' = queued - 1
  /\ UNCHANGED <<pc, call, cancelled, approval, allowed, tool, lateStart>>

HostAnswers == approval = "pending" /\ approval' \in {"allow", "deny", "error"}
  /\ UNCHANGED <<pc, call, cancelled, allowed, tool, queued, lateStart>>

ToolSettles == tool = "running" /\ tool' \in {"ok", "err"}
  /\ UNCHANGED <<pc, call, cancelled, approval, allowed, queued, lateStart>>

\* ---- run() ----

\* emit(ToolCall).
ReportCall == pc = "emitCall" /\ Send /\ pc' = "emitAsked"
  /\ UNCHANGED <<call, cancelled, approval, allowed, tool, lateStart>>

\* emit(ApprovalAsked), then host.approve() with no await between.
AskApproval == pc = "emitAsked" /\ Send
  /\ approval' = "pending" /\ pc' = "awaitApproval"
  /\ lateStart' = (lateStart \/ cancelled)
  /\ UNCHANGED <<call, cancelled, allowed, tool>>

\* hosted(approve): the callback settled. An error is a denial.
TakeAnswer == pc = "awaitApproval" /\ MayProceed
  /\ approval \in {"allow", "deny", "error"}
  /\ allowed' = (approval = "allow") /\ pc' = "emitAnswered"
  /\ UNCHANGED <<call, cancelled, approval, tool, queued, lateStart>>

\* hosted(): cancel wins the select; host.abort() is called.
AbandonApproval == pc = "awaitApproval" /\ cancelled
  /\ approval' = (IF approval = "pending" THEN "aborted" ELSE approval)
  /\ pc' = "finish"
  /\ UNCHANGED <<call, cancelled, allowed, tool, queued, lateStart>>

\* emit(ApprovalAnswered), then host.call_tool() with no await between, when allowed.
AnswerAllowed == pc = "emitAnswered" /\ allowed /\ Send
  /\ tool' = "running" /\ pc' = "awaitTool"
  /\ lateStart' = (lateStart \/ cancelled)
  /\ UNCHANGED <<call, cancelled, approval, allowed>>

AnswerDenied == pc = "emitAnswered" /\ ~allowed /\ Send
  /\ pc' = "emitDenied"
  /\ UNCHANGED <<call, cancelled, approval, allowed, tool, lateStart>>

ReportDenied == pc = "emitDenied" /\ Send /\ NextCall
  /\ UNCHANGED <<cancelled, lateStart>>

TakeResult == pc = "awaitTool" /\ MayProceed
  /\ tool \in {"ok", "err"} /\ pc' = "emitResult"
  /\ UNCHANGED <<call, cancelled, approval, allowed, tool, queued, lateStart>>

\* hosted(): the timeout wins the select; host.abort() is called; the run goes on.
ToolTimesOut == pc = "awaitTool" /\ MayProceed
  /\ tool = "running" /\ tool' = "aborted" /\ pc' = "emitResult"
  /\ UNCHANGED <<call, cancelled, approval, allowed, queued, lateStart>>

AbandonTool == pc = "awaitTool" /\ cancelled
  /\ tool' = (IF tool = "running" THEN "aborted" ELSE tool)
  /\ pc' = "finish"
  /\ UNCHANGED <<call, cancelled, approval, allowed, queued, lateStart>>

ReportResult == pc = "emitResult" /\ Send /\ NextCall
  /\ UNCHANGED <<cancelled, lateStart>>

\* emit(): cancel wins the select.
StopAtEmit == pc \in Emits /\ cancelled /\ pc' = "finish"
  /\ UNCHANGED <<call, cancelled, approval, allowed, tool, queued, lateStart>>

\* start(): tx.send(RunFinished). Not raced against cancel.
SendFinished == pc = "finish" /\ queued < Capacity
  /\ queued' = queued + 1 /\ pc' = "done"
  /\ UNCHANGED <<call, cancelled, approval, allowed, tool, lateStart>>

Done == pc = "done" /\ queued = 0 /\ UNCHANGED vars

Next ==
  \/ BuilderCancels \/ BuilderReadsEvent \/ HostAnswers \/ ToolSettles
  \/ ReportCall \/ AskApproval \/ TakeAnswer \/ AbandonApproval
  \/ AnswerAllowed \/ AnswerDenied \/ ReportDenied
  \/ TakeResult \/ ToolTimesOut \/ AbandonTool \/ ReportResult
  \/ StopAtEmit \/ SendFinished \/ Done

Spec == Init /\ [][Next]_vars

\* ---- rules ----

TypeOK ==
  /\ pc \in Emits \cup {"awaitApproval", "awaitTool", "finish", "done"}
  /\ call \in 1..Calls /\ cancelled \in BOOLEAN /\ allowed \in BOOLEAN
  /\ approval \in {"none", "pending", "allow", "deny", "error", "aborted"}
  /\ tool \in {"none", "running", "ok", "err", "aborted"}
  /\ queued \in 0..Capacity /\ lateStart \in BOOLEAN

\* Once the builder has cancelled, the loop starts no new approval prompt and no new tool.
NoHostCallStartsAfterCancel == ~lateStart

\* A tool runs only if its approval handler answered allow.
ToolRunsOnlyWhenAllowed == tool # "none" => approval = "allow"

\* When the run is over, no host callback is left running without an abort.
NoOrphanHostCall == pc \in {"finish", "done"} => (approval # "pending" /\ tool # "running")
====
