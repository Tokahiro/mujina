// Steam on Windows reports the WLAN device without access points, so Big Picture's Wi-Fi icon is
// grey: this appends one to every device update. indicator.rs runs it as (<this file>)(version).
(function (version) {
  // Versioned: a context may still carry the hook of an older Mujina, which cannot adopt. Set
  // only once hook() has succeeded, so that a failed hook is tried again.
  if (window.__steamWifiHooked === version) return 'in place';
  // An earlier run is still waiting for Steam's UI; a second one could subscribe a second time.
  if (window.__steamWifiWaiting === version) return 'waiting';
  window.__steamWifi = window.__steamWifi || { ssid: 'Wi-Fi', strength: 3 };

  function rv(b, p) { let r = 0, s = 0, x; do { x = b[p++]; r += (x & 127) * Math.pow(2, s); s += 7; } while (x & 128); return [r, p]; }
  function wv(n) { const o = []; do { let x = n % 128; n = Math.floor(n / 128); if (n) x |= 128; o.push(x); } while (n); return o; }
  function fields(b) {
    const out = []; let p = 0;
    while (p < b.length) {
      const hs = p; let [k, q] = rv(b, p); const n = Math.floor(k / 8), wt = k % 8; p = q; let vs = p;
      if (wt === 0) { [, p] = rv(b, p); }
      else if (wt === 2) { const [l, q2] = rv(b, p); vs = q2; p = q2 + l; }
      else if (wt === 5) { p += 4; }
      else if (wt === 1) { p += 8; }
      else throw new Error('wiretype ' + wt);
      out.push({ n, wt, start: hs, end: p, vs, ve: p });
    }
    return out;
  }
  function ld(n, payload) { return [...wv(n * 8 + 2), ...wv(payload.length), ...payload]; }
  function vi(n, v) { return [...wv(n * 8), ...wv(v)]; }
  function apBytes() {
    const s = window.__steamWifi; const ssid = [...new TextEncoder().encode(s.ssid || 'Wi-Fi')];
    return [...vi(1, 1), ...vi(2, s.strength == null ? 3 : s.strength), ...ld(3, ssid), ...vi(4, 1), ...vi(5, 1), ...vi(6, 0), ...vi(12, 1)];
  }
  function patch(buf) {
    const b = new Uint8Array(buf); const out = [];
    for (const f of fields(b)) {
      if (f.n === 1 && f.wt === 2) {
        const d = b.subarray(f.vs, f.ve); const df = fields(d);
        const get = n => { const x = df.find(z => z.n === n); return x ? rv(d, x.vs)[0] : null; };
        if (get(2) === 2 && get(3) === 5) {
          const nd = []; let hadWl = false;
          for (const g of df) {
            if (g.n === 10 && g.wt === 2) {
              hadWl = true; const w = d.subarray(g.vs, g.ve);
              if (fields(w).some(z => z.n === 1)) nd.push(...d.subarray(g.start, g.end));
              else nd.push(...ld(10, [...w, ...ld(1, apBytes())]));
            } else nd.push(...d.subarray(g.start, g.end));
          }
          if (!hadWl) nd.push(...ld(10, ld(1, apBytes())));
          out.push(...ld(1, nd)); continue;
        }
      }
      out.push(...b.subarray(f.start, f.end));
    }
    return new Uint8Array(out).buffer;
  }
  let register = null;
  function hook() {
    const N = window.SteamClient && window.SteamClient.System && window.SteamClient.System.Network;
    if (!N) return false;
    // Wrapped already, by an older hook: its wrapper is the way to subscribe then.
    if (N.__wifiHooked) { register = register || N.RegisterForDeviceChanges.bind(N); return true; }
    const orig = N.RegisterForDeviceChanges.bind(N);
    register = orig;
    N.RegisterForDeviceChanges = function (cb) {
      // Known from now on, not only from the first update: adopt() must not subscribe as well.
      window.__steamWifiSubscribed = true;
      return orig(function (b) {
        window.__steamWifiLast = b; window.__steamWifiCb = cb;
        let pb; try { pb = patch(b); } catch (e) { pb = b; window.__steamWifiError = String(e); }
        return cb(pb);
      });
    };
    N.__wifiHooked = true; return true;
  }
  window.__steamWifiRefire = function () {
    if (window.__steamWifiCb && window.__steamWifiLast) { try { window.__steamWifiCb(patch(window.__steamWifiLast)); } catch (e) {} }
  };
  // An earlier subscriber's callback cannot be captured, but it is a method of the UI's network
  // store: subscribe too and hand the store patched data after Steam's, instead of a reload.
  function adopt() {
    if (window.__steamWifiCb || window.__steamWifiSubscribed) return true;
    const S = window.SystemNetworkStore;
    if (!register || !S || typeof S.OnNetworkDevicesChanged !== 'function') return false;
    const cb = function (pb) { return S.OnNetworkDevicesChanged(pb); };
    register(function (b) {
      window.__steamWifiLast = b;
      try { cb(patch(b)); } catch (e) { window.__steamWifiError = String(e); }
    });
    window.__steamWifiCb = cb;
    window.__steamWifiAdopted = true;
    return true;
  }
  // SteamClient and the store appear after the document: retry for ~30 s. Only the last attempt's
  // error is kept, so a run that merely waited does not look failed.
  let hooked = false;
  function settle() {
    window.__steamWifiError = null;
    try {
      if (!hooked) {
        if (!hook()) return false;
        hooked = true;
        window.__steamWifiHooked = version;
      }
      return adopt();
    } catch (e) {
      window.__steamWifiError = String(e);
      return false;
    }
  }
  if (settle()) return 'ready';
  window.__steamWifiWaiting = version;
  let n = 0;
  const t = setInterval(function () {
    if (settle() || ++n > 600) { clearInterval(t); window.__steamWifiWaiting = 0; }
  }, 50);
  return 'waiting';
})
