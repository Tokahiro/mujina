# ADR-0003: `windows-sys`, and run-time binding for the Xbox mode API

Status: accepted; amended by ADR-0014: COM is initialised where a Win32 call Mujina makes asks for
it (an STA for `ShellExecuteW`). The binding stays `windows-sys`.

## Decision

- Use `windows-sys` (raw bindings) instead of `windows`: Mujina needs no COM or WinRT, and the
  raw crate compiles faster and adds nothing to the binary.
- All calls are wrapped once, in `mujina-winutil` or in the adapter that owns the concept, with
  a `// SAFETY:` contract. Inner rings forbid `unsafe`.
- The full screen experience API (`api-ms-win-gaming-experience-l1-1-0`) is absent from
  `windows-sys` and from older Windows builds. It is bound at run time with
  `LoadLibraryExW`/`GetProcAddress`; absence is a normal state (`FseState::Unavailable`), not an
  error.
