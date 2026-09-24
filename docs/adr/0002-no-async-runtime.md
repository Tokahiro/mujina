# ADR-0002: No async runtime

Status: accepted; the limit of three threads superseded by ADR-0014, which states the invariant it
stood for: every thread is blocked in the kernel while idle, as the agent's `cost:` line shows.

## Context

Everything Mujina waits for is a Win32 kernel object or window message. A low-level keyboard
hook additionally requires a thread that pumps messages and answers within a few hundred
milliseconds, or Windows silently removes the hook.

## Decision

No tokio or other runtime. The agent's main thread runs a message loop on
`MsgWaitForMultipleObjectsEx(INFINITE)`; the few blocking jobs (timed key holds, the debugging
socket) get a dedicated thread each that is itself blocked on an event. CLI parsing uses
`lexopt`, logging the `log` facade with a synchronous file writer.

## Consequences

- Idle means idle: no timer wheel, no worker pool, no periodic wake-ups.
- Small binary and working set, fast start.
- Concurrency is explicit and limited to message passing between at most three threads.
