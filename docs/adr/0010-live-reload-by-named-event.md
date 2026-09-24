# ADR-0010: A running agent reloads its settings when a named event is signalled

Status: accepted; the fixed list of settings that wait superseded by ADR-0013: whether a change
applies at once is part of each setting's description, core and launchers' alike. Since stage 6
the device takes a change over through `DeviceButtons::reconfigure` (ADR-0013): another
key-chord device, a button of one's own or none at once, a device of another crate next session.

## Context

Until 0.17.0 every setting took effect the next time Xbox mode was entered, because the agent
reads `config.toml` once at start. A settings GUI that changes something and then says "leave
Xbox mode and come back" is not one people want to use. Options were restarting the agent on
every change, watching the file, or telling the agent.

Restarting drops the keyboard hook and the Steam debugging session for a moment. Watching the
data directory wakes the agent on every write of its own log, and filtering by file name needs
overlapped directory reads. Idle cost is a design goal (ADR-0002): whatever is chosen must cost
nothing while nothing changes.

## Decision

- A named auto-reset event, `Local\Mujina.settings`, in the session's namespace. The agent
  waits on it next to its other handles; `mujinactl config set/unset` (and later the GUI) signal
  it after a successful write. No IPC channel, no message format: the file is the message
  (ADR-0009).
- On the signal the agent re-reads the file through a `SettingsSource` port and the use case
  applies what can change under a running session: chords, timing, exit policy, the game start
  screen, the device button (switched off, on, or to another chord, by moving the keyboard hook),
  and the log level.
- Deliberately not live: the launcher kind and `[launcher.generic]` (home role and agent must
  agree within a session, ADR-0007), the Wi-Fi indicator (it needs the launcher started with its
  debugging marker), and the launch screen (the home role reads it at each activation anyway).
  `takes_effect_live` in the application ring names these, so tools can say so.
- Hand edits of the file are not noticed until the next session, as before.

## Consequences

- One more handle in the agent's wait set; nothing runs until it is signalled.
- The keyboard hook now exists only while there is a button to intercept, set through
  `InputInterceptor::set_trigger`, instead of being installed once at start.
- Any process of the session can make the agent re-read its own configuration. That is harmless:
  it can only make the agent do what the file already says.
