# ADR-0006: The Wi-Fi indicator is fed through one persistent debugging session

Status: accepted; where it lives changed in stage 5 (ADR-0013, ADR-0014): the whole feature is the
Steam adapter's. The WLAN reader feeds the `steam-ui` worker directly instead of waking the agent,
the hysteresis is the adapter's rather than the domain's, and the same worker also carries out the
device button's direct menu presses, over a second session of their own. Still to do (rings-4):
since only that worker keeps a link, the direct menus need the Wi-Fi fix on as well as `ui_link`;
with the fix off the device button opens Steam's menus by their shortcuts. They should need only
`ui_link`, once the worker can run without the Wi-Fi hook and without a thread more.

## Context

Steam on Windows reports the WLAN adapter as connected but with an empty access-point list, and
Big Picture draws that as "disconnected". Valve implements Wi-Fi management only for SteamOS.
The list the UI receives can be patched from inside Steam's shared JavaScript context, reachable
through the Chromium debugging port that Steam opens when `.cef-enable-remote-debugging` exists
in its folder.

## Decision

- `mujina.exe` (home role) makes sure the marker exists on every activation. Steam reads it only
  when it starts, so a marker created while Steam runs takes effect after the next Steam start
  (the log says so).
- The agent's `steam-ui` thread connects as early in Steam's start-up as the debugging port
  allows, registers `assets/hook.js` with `Page.addScriptToEvaluateOnNewDocument` **and runs it
  in the current document at once**. The hook only catches a subscription made after it is in
  place; injected this early it usually beats the UI, and nothing visible happens.
- Reloading Steam's UI replays its whole start-up, video included, so *when* matters more than
  *whether*. While Steam starts, the debugging port is probed back to back (bounded to the first
  30 s of a round), so the hook goes in within a fraction of a second of the script context
  existing. If the UI subscribed first all the same, it is reloaded at once: that early, seconds
  before Big Picture is shown, nobody sees it. Tried and rejected on a device: delaying the
  reload by 20 s to protect the start-up video. It looked like Steam starting a second time.
- Tried and rejected on a device (0.7.0): avoiding the reload by having the browser hold new
  pages at birth (`Target.setAutoAttach` with `waitForDebuggerOnStart` on a browser-level
  session), so that the hook is registered ahead of Steam's own script. Steam's browser
  announces its pages as type `other` and refuses `Page.addScriptToEvaluateOnNewDocument` for
  them, and the second page held this way, the Big Picture window, never resumes: not on
  `Runtime.runIfWaitingForDebugger`, not when the session closes. The result is a black screen
  until Steam is killed.
- Since 0.10.1 the reload is a last resort. A UI that subscribed before the hook arrived cannot
  have its callback captured any more, but that callback is a method of the UI's network store
  (`SystemNetworkStore.OnNetworkDevicesChanged`). The hook subscribes to device changes itself
  and hands the store the patched data right after Steam handed it the unpatched. Tried against
  a Steam that had been running without the hook: the store showed the pushed network at once.
  This matters most when Xbox mode is entered with Steam already running, where a reload is in
  plain sight: Big Picture vanishes, the desktop shows, Windows activates the home app again.
  The UI is reloaded only if it is up and has no such store.
- The thread then **keeps that one session open**. A registered script lives exactly as long as
  its session, so holding the session makes the patch survive UI reloads; no periodic re-check
  is needed.
- Wi-Fi changes come from `WlanRegisterNotification`. The callback wakes the agent only on
  connect, disconnect, or when the signal moved into another bar bucket; the domain adds
  hysteresis and pushes only real changes.
- Always `127.0.0.1`: Steam does not listen on `::1`.

## Consequences

- Zero traffic and zero wake-ups while the signal is stable; normally no UI reload at all.
- The debugging port is unauthenticated for local programs (see SECURITY.md).
- `hook.js` depends on undocumented Steam internals (protobuf field numbers). When Valve changes
  them, the patch falls back to the unmodified data and the icon is grey again; nothing else is
  affected.
