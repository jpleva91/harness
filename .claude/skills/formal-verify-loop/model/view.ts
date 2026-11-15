export const title = "One run of the loop: approvals, tools and cancel";

const where: Record<string, string> = {
  emitAsked: "Reporting a tool call",
  awaitApproval: "Waiting for approval",
  emitAnswered: "Reporting the approval answer",
  emitDenied: "Reporting a denied tool",
  awaitTool: "Tool running",
  emitResult: "Reporting the tool result",
  finish: "Ending the run",
  done: "Run over",
};

export const state = (v: any) => {
  const base = where[v.pc];
  if (v.lateStart) return `${base} (a callback started after cancel)`;
  return v.cancelled && v.pc !== "done" ? `${base}, cancel requested` : base;
};

const labels: Record<string, string | null> = {
  BuilderCancels: "the builder cancels",
  BuilderReadsEvent: null,
  HostAnswers: null,
  ToolSettles: null,
  AskApproval: "the loop asks for approval",
  TakeAnswer: "the approval handler answers",
  AbandonApproval: "the loop drops the approval and aborts it",
  AnswerAllowed: "allowed: the loop starts the tool",
  AnswerDenied: "denied",
  ReportDenied: "the denial is reported",
  TakeResult: "the tool returns",
  ToolTimesOut: "the tool times out and is aborted",
  AbandonTool: "the loop drops the tool and aborts it",
  ReportResult: "the result is reported",
  StopAtEmit: "the loop sees the cancel",
  SendFinished: "run_finished is sent",
  Done: null,
};

export const step = (action: string) => labels[action] ?? null;
