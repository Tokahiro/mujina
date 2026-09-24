//! Wi-Fi state from the WLAN service, with change notifications instead of polling, fed to the
//! Steam UI worker that corrects Big Picture's Wi-Fi icon.
//!
//! Everything that talks to the WLAN service happens on a thread of its own, never on the
//! agent's main thread nor on the Steam UI worker. Since Windows 11 24H2 the name and signal of
//! the current network are guarded by the location permission, and the first query makes
//! Windows show its consent prompt and blocks until the user answers. A main thread blocked like
//! that cannot service the keyboard hook, and Windows removes low-level hooks that do not answer;
//! a Steam UI worker blocked like that would miss the moment to hook Steam's UI early.
//!
//! The thread is blocked in the kernel unless the WLAN service reports a change that can alter
//! what the icon shows: connect, disconnect, or a signal quality that moved into another bar
//! bucket. Drivers report signal quality every few seconds; reacting to each report would
//! defeat the purpose. It hands what the icon is to show straight to the Steam UI worker; the
//! agent's event loop is not woken for it.
//!
//! Connecting and disconnecting are the service's ACM notifications, the signal quality its MSM
//! ones. Windows refuses a registration for MSM (ERROR_ACCESS_DENIED) unless the app has the
//! wiFiControl device capability, which the package declares (`packaging/AppxManifest.xml.in`)
//! and which "will require consent from the user regarding access to location" (Microsoft, on
//! WlanRegisterNotification). So the first query, which may ask for that consent, comes before
//! the registration, and a refused MSM leaves ACM alone: the icon then follows connecting and
//! disconnecting, and keeps the bars read at the last of them. That query asks only when Wi-Fi
//! is connected; a refusal that came before any reading with the connection's details is tried
//! again once, at the first such reading, when the consent has been given (`SignalRetry`).
//!
//! The calls to the service themselves are in `mujina_winutil::wlan`. Opened only while Steam's
//! Wi-Fi fix is on, so that no other launcher's user is asked for the location permission.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;

use mujina_winutil::error::Win32Error;
use mujina_winutil::event::Event;
use mujina_winutil::location::{self, LocationConsent};
use mujina_winutil::wlan::{Notification, WlanClient};
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_MSM,
};
use windows_sys::core::GUID;

use crate::indicator::SteamWifiIndicator;
use crate::wifi::{
    Followed, IconFeed, SignalBars, SignalRetry, WifiChanges, WifiReading, register_changes,
};

/// `wlan_notification_acm_connection_complete`
const ACM_CONNECTION_COMPLETE: u32 = 10;
/// `wlan_notification_acm_disconnected`
const ACM_DISCONNECTED: u32 = 21;
/// `wlan_notification_msm_signal_quality_change`
const MSM_SIGNAL_QUALITY_CHANGE: u32 = 8;

/// Shown when the user declined the location permission: connected, details unknown.
const UNKNOWN_NETWORK: &str = "Wi-Fi";
const UNKNOWN_QUALITY: u8 = 60;

/// Starts the worker that feeds `icon`. `false` only if the thread or its event cannot be
/// created; a missing WLAN service merely means there is never a reading, which the worker says.
pub fn follow(icon: SteamWifiIndicator) -> bool {
    let Ok(wake) = Event::new() else {
        return false;
    };
    let worker = Worker {
        wake: Arc::new(wake),
        icon,
    };
    thread::Builder::new()
        .name("wlan".into())
        .spawn(move || worker.run())
        .is_ok()
}

/// Decides which notifications wake the worker.
struct Relevance {
    /// Bars of the last signal quality that woke the worker; 255 = none yet.
    last_bars: AtomicU8,
}

impl Relevance {
    fn new() -> Self {
        Self {
            last_bars: AtomicU8::new(u8::MAX),
        }
    }

    /// Whether the notification can change what the indicator shows.
    fn wakes(&self, notification: &Notification<'_>) -> bool {
        match (notification.source, notification.code) {
            (WLAN_NOTIFICATION_SOURCE_ACM, ACM_CONNECTION_COMPLETE | ACM_DISCONNECTED) => {
                self.last_bars.store(u8::MAX, Ordering::Relaxed);
                true
            }
            (WLAN_NOTIFICATION_SOURCE_MSM, MSM_SIGNAL_QUALITY_CHANGE) => {
                // For this notification the data is a ULONG signal quality.
                let Some(quality) = notification.data.first_chunk::<4>() else {
                    return false;
                };
                let quality = u32::from_ne_bytes(*quality);
                let bars =
                    SignalBars::from_quality(u8::try_from(quality.min(100)).unwrap_or(100)).get();
                self.last_bars.swap(bars, Ordering::Relaxed) != bars
            }
            _ => false,
        }
    }
}

struct Worker {
    /// Signalled by the WLAN notification callback.
    wake: Arc<Event>,
    icon: SteamWifiIndicator,
}

impl Worker {
    fn run(self) {
        // Opened here: the client is used on this thread only, and it lives as long as the
        // thread, which the agent never ends.
        let mut client = match WlanClient::open() {
            Ok(client) => client,
            Err(error) => {
                log::warn!(
                    "WLAN service not available ({error}); the Wi-Fi icon stays as Steam draws it"
                );
                return;
            }
        };
        let mut icon = Icon::new(self.icon);
        // May block on the location consent prompt the first time; that is why it is on this
        // thread. Before the registration, so that a consent given at the prompt counts for the
        // signal strength's notifications too.
        let detailed = icon.refresh(&client);
        let consent = location::consent();
        let followed = follow_changes(&mut client, &self.wake);
        say_followed(&followed, consent);
        let mut retry = SignalRetry::after_first(&followed, detailed);
        loop {
            // The first time round, again at once: a change between the first query and the
            // registration came with no notification.
            let detailed = icon.refresh(&client);
            if retry.due(detailed) {
                let followed = follow_changes(&mut client, &self.wake);
                let now = location::consent();
                if news(&followed, consent, now) {
                    say_followed(&followed, now);
                }
                // Again: `notify` unregisters before it registers, and a change in between came
                // with no notification.
                icon.refresh(&client);
            }
            // Cannot fail on an event this worker owns; if it did, waiting again would spin.
            if let Err(error) = self.wake.wait() {
                log::warn!("Wi-Fi changes are no longer followed ({error})");
                return;
            }
        }
    }
}

/// Registers for the changes that can alter the icon, as far as Windows grants them, in place of
/// any registered before.
fn follow_changes(client: &mut WlanClient, wake: &Arc<Event>) -> Followed<Win32Error> {
    register_changes(
        |changes| {
            // A fresh one for each registration: at worst the first signal after it wakes the
            // worker for bars the icon shows already, and the query finds nothing to change.
            let relevance = Relevance::new();
            let wake = Arc::clone(wake);
            client.notify(sources(changes), move |notification| {
                if relevance.wakes(notification) {
                    wake.set();
                }
            })
        },
        refused,
    )
}

/// Whether the outcome of registering again is worth a line. A refusal again was said already,
/// unless the permission was given since: then it points at the package, which the first line
/// did not say.
fn news(followed: &Followed<Win32Error>, then: LocationConsent, now: LocationConsent) -> bool {
    match followed {
        Followed::ConnectionOnly { .. } => {
            now == LocationConsent::Granted && then != LocationConsent::Granted
        }
        Followed::All | Followed::Nothing(_) => true,
    }
}

/// Says which changes are followed. `consent` is the location permission as Windows' settings
/// have it at the registration: with it, a refusal points at the package; without it, a refusal
/// is expected and costs nothing.
fn say_followed(followed: &Followed<Win32Error>, consent: LocationConsent) {
    match followed {
        Followed::All => {
            log::info!("Wi-Fi changes followed: connecting, disconnecting and signal strength");
        }
        Followed::ConnectionOnly { refusal } => match consent {
            LocationConsent::Granted => log::warn!(
                "Wi-Fi changes followed: connecting and disconnecting only; Windows refuses \
                 signal strength changes ({refusal}) although location is allowed for Mujina: \
                 the package's wiFiControl capability is missing or not honoured"
            ),
            // Only at info: the reading is then the generic one, whose quality is fixed, so the
            // signal's changes could not move the icon, and its warning is logged already.
            LocationConsent::DeniedForApp | LocationConsent::DeniedEverywhere => log::info!(
                "Wi-Fi changes followed: connecting and disconnecting; without the location \
                 permission Windows refuses signal strength changes ({refusal}), which the \
                 generic icon would not show anyway"
            ),
            // Expected when Wi-Fi was not connected yet, so nothing asked: tried again (`run`).
            LocationConsent::NotAsked => log::info!(
                "Wi-Fi changes followed: connecting and disconnecting for now; Windows refuses \
                 signal strength changes ({refusal}) before the location permission is given"
            ),
            LocationConsent::Unpackaged => log::warn!(
                "Wi-Fi changes followed: connecting and disconnecting only; Windows refuses \
                 signal strength changes ({refusal}) unless Mujina has the location permission \
                 (Settings > Privacy & security > Location) and its package the wiFiControl \
                 capability"
            ),
        },
        Followed::Nothing(error) => {
            log::warn!("Wi-Fi change notifications unavailable ({error})");
        }
    }
}

/// Whether the service refused a registration rather than failed at it: what Microsoft
/// documents for MSM without the wiFiControl capability.
fn refused(error: &Win32Error) -> bool {
    error.code == ERROR_ACCESS_DENIED
}

/// The WLAN service's notification sources for `changes`.
fn sources(changes: WifiChanges) -> u32 {
    match changes {
        WifiChanges::Connection => WLAN_NOTIFICATION_SOURCE_ACM,
        WifiChanges::ConnectionAndSignal => {
            WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM
        }
    }
}

/// The icon as this worker last fed it.
struct Icon {
    indicator: SteamWifiIndicator,
    latest: Option<WifiReading>,
    feed: IconFeed,
    /// Whether the missing location permission was logged.
    permission_noted: bool,
}

impl Icon {
    fn new(indicator: SteamWifiIndicator) -> Self {
        Self {
            indicator,
            latest: None,
            feed: IconFeed::default(),
            permission_noted: false,
        }
    }

    /// Asks for the connection, and shows it if that changes what the icon shows. `true` if
    /// Windows gave the connection's details, which it does only with the location permission.
    fn refresh(&mut self, client: &WlanClient) -> bool {
        let answer = query(client, &mut self.permission_noted);
        let detailed = answer.as_ref().is_some_and(|answer| answer.detailed);
        let reading = answer.map(|answer| answer.reading);
        if reading != self.latest {
            if let Some(status) = self.feed.next(reading.as_ref()) {
                self.indicator.show(&status);
            }
            self.latest = reading;
        }
        detailed
    }
}

/// A reading, and where it came from.
struct Answer {
    reading: WifiReading,
    /// From the connection's details; `false` for the generic reading shown without the
    /// location permission.
    detailed: bool,
}

/// The connection of the first connected Wi-Fi interface.
fn query(client: &WlanClient, permission_noted: &mut bool) -> Option<Answer> {
    let interfaces = client.interfaces().ok()?;
    interfaces
        .iter()
        .filter(|interface| interface.connected)
        .find_map(|interface| reading(client, &interface.guid, permission_noted))
}

/// The connection of one interface; a generic one without the location permission.
fn reading(client: &WlanClient, interface: &GUID, permission_noted: &mut bool) -> Option<Answer> {
    match client.current_connection(interface) {
        Ok(connection) => Some(Answer {
            reading: WifiReading {
                ssid: String::from_utf8_lossy(&connection.ssid).into_owned(),
                quality: u8::try_from(connection.signal_quality.min(100)).unwrap_or(100),
            },
            detailed: true,
        }),
        Err(error) if error.code == ERROR_ACCESS_DENIED => {
            if !*permission_noted {
                *permission_noted = true;
                log::warn!(
                    "Windows denies Wi-Fi details without the location permission; the \
                     indicator shows a generic connection (Settings > Privacy & security > \
                     Location)"
                );
            }
            Some(Answer {
                reading: WifiReading {
                    ssid: UNKNOWN_NETWORK.to_string(),
                    quality: UNKNOWN_QUALITY,
                },
                detailed: false,
            })
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(source: u32, code: u32, data: &[u8]) -> Notification<'_> {
        Notification { source, code, data }
    }

    fn quality(value: u32) -> [u8; 4] {
        value.to_ne_bytes()
    }

    #[test]
    fn connecting_and_disconnecting_always_wake_the_worker() {
        let relevance = Relevance::new();
        let connected = notification(WLAN_NOTIFICATION_SOURCE_ACM, ACM_CONNECTION_COMPLETE, &[]);
        let disconnected = notification(WLAN_NOTIFICATION_SOURCE_ACM, ACM_DISCONNECTED, &[]);
        assert!(relevance.wakes(&connected));
        assert!(relevance.wakes(&connected));
        assert!(relevance.wakes(&disconnected));
    }

    #[test]
    fn a_signal_wakes_the_worker_only_when_its_bars_change() {
        assert_eq!(
            SignalBars::from_quality(95),
            SignalBars::from_quality(99),
            "the test needs two qualities in one bucket"
        );
        let relevance = Relevance::new();
        let (strong, also_strong, weak) = (quality(95), quality(99), quality(10));
        let change = |data| {
            notification(
                WLAN_NOTIFICATION_SOURCE_MSM,
                MSM_SIGNAL_QUALITY_CHANGE,
                data,
            )
        };
        assert!(relevance.wakes(&change(&strong)), "the first reading");
        assert!(!relevance.wakes(&change(&also_strong)), "same bars");
        assert!(relevance.wakes(&change(&weak)));
        // A connection starts over: the same bars wake the worker again after it.
        relevance.wakes(&notification(
            WLAN_NOTIFICATION_SOURCE_ACM,
            ACM_CONNECTION_COMPLETE,
            &[],
        ));
        assert!(relevance.wakes(&change(&weak)));
    }

    #[test]
    fn only_access_denied_counts_as_a_refusal() {
        let error = |code| Win32Error {
            call: "WlanRegisterNotification",
            code,
        };
        assert!(refused(&error(ERROR_ACCESS_DENIED)));
        // RPC_S_SERVER_UNAVAILABLE: the service is gone, and asking for less would not help.
        assert!(!refused(&error(1722)));
    }

    #[test]
    fn registering_again_is_logged_unless_it_repeats_what_was_said() {
        use LocationConsent::{Granted, NotAsked};
        let refused = Followed::ConnectionOnly {
            refusal: Win32Error {
                call: "WlanRegisterNotification",
                code: ERROR_ACCESS_DENIED,
            },
        };
        assert!(news(&Followed::All, NotAsked, Granted));
        assert!(news(&refused, NotAsked, Granted), "now it is the package");
        assert!(!news(&refused, Granted, Granted), "the package was named");
        assert!(!news(&refused, NotAsked, NotAsked), "said already");
    }

    #[test]
    fn the_signal_is_asked_for_together_with_the_connection() {
        assert_eq!(
            sources(WifiChanges::Connection),
            WLAN_NOTIFICATION_SOURCE_ACM
        );
        assert_eq!(
            sources(WifiChanges::ConnectionAndSignal),
            WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM
        );
    }

    #[test]
    fn other_notifications_and_short_data_do_not_wake_the_worker() {
        let relevance = Relevance::new();
        assert!(!relevance.wakes(&notification(
            WLAN_NOTIFICATION_SOURCE_MSM,
            MSM_SIGNAL_QUALITY_CHANGE,
            &[1, 2, 3],
        )));
        assert!(!relevance.wakes(&notification(WLAN_NOTIFICATION_SOURCE_ACM, 1, &[])));
        assert!(!relevance.wakes(&notification(
            WLAN_NOTIFICATION_SOURCE_MSM,
            ACM_CONNECTION_COMPLETE,
            &quality(50),
        )));
    }
}
