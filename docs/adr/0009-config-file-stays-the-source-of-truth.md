# ADR-0009: `config.toml` stays the source of truth, and tools edit it in place

Status: accepted; amended in the review of September 2026 (reading is strict per key)

## Context

A settings GUI and `mujinactl` need to change settings. Options were a store of their own (the
registry, a JSON file the GUI owns, settings kept by the agent and changed over IPC) or editing
the file users already edit by hand.

A second store would need a rule for which one wins, and would hide settings from people who
read the file. Rewriting the file from a data structure would drop the comments that make it
self-explaining.

## Decision

- `config.toml` remains the only store. A write port, `SettingsStore::apply(&[SettingChange])`,
  sits next to the read port `SettingsSource`. A change names a key by its dotted path in the
  file (`features.launch_screen`) and carries a value or `None` for "back to the default".
- `adapter-config` implements it with `toml_edit`, which keeps comments and layout. Setting a key
  the template shows commented out turns that line into the setting and keeps its explanation;
  unsetting comments it out again. New sections go to the end of the file.
- Before anything is written, the result goes through the same strict parser and resolver that
  read the file. A change that would be rejected (unknown key, wrong type) or remarked on (a
  note such as an unknown chord) is refused with that reason; the file is replaced in one step.
  There is exactly one definition of what a valid setting is.
- Reading is strict per key, not per file (amended in the review of September 2026): an unknown
  or mistyped key is skipped with a note of its own and every other setting still applies. Only
  a file that is not valid TOML is ignored as a whole. Writers hold `config.toml.lock` while
  they read, check and replace the file, so Mujina Settings and `mujinactl` never interleave.
- Changes in one `apply` are validated together, so settings that only make sense as a group
  (`kind = "generic"` and its `[launcher.generic]`) can be made in one step.

## Consequences

- Users, `mujinactl config set` and the GUI all see and change the same file; hand edits and
  tool edits mix freely.
- A file that no longer parses is not touched by tools; they ask for it to be fixed by hand.
- Values with a meaning beyond their type (a chord, a device profile id) are validated by the
  reader's notes, not by a second schema.
- How a running agent learns about a change is a separate decision (live reload).
