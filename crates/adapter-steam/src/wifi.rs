//! The Wi-Fi status as Big Picture's icon wants to see it: bars smoothed at their edges, and
//! pushed only when they, or the network, change; and which of the WLAN service's changes are
//! followed when Windows refuses some, and when to ask again. Portable, so these rules are tested
//! on every system; the reading itself and the registration come from the WLAN service
//! (`wlan.rs`).

/// Signal strength in the 0–4 scale Big Picture draws (none, weak, ok, good, excellent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignalBars(u8);

impl SignalBars {
    /// Maps a 0–100 link quality onto bars.
    pub const fn from_quality(quality: u8) -> Self {
        Self(match quality {
            0 => 0,
            1..=25 => 1,
            26..=50 => 2,
            51..=75 => 3,
            _ => 4,
        })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Smooths link quality into bars. A quality hovering around a bucket edge would otherwise flip
/// the icon (and wake Steam's UI) on every driver notification.
#[derive(Debug, Default)]
pub struct BarsFilter {
    current: Option<SignalBars>,
}

impl BarsFilter {
    /// A change of bucket only counts once the quality is this far past the edge.
    const MARGIN: u8 = 3;

    pub fn update(&mut self, quality: u8) -> SignalBars {
        let raw = SignalBars::from_quality(quality);
        let settled = match self.current {
            Some(current) if current != raw && quality != 0 => {
                let nudged = if raw > current {
                    quality.saturating_sub(Self::MARGIN)
                } else {
                    quality.saturating_add(Self::MARGIN)
                };
                if SignalBars::from_quality(nudged) == current {
                    current
                } else {
                    raw
                }
            }
            _ => raw,
        };
        self.current = Some(settled);
        settled
    }

    /// Forgets the history, e.g. after a disconnect.
    pub fn reset(&mut self) {
        self.current = None;
    }
}

/// What the icon should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiStatus {
    Disconnected,
    Connected { ssid: String, bars: SignalBars },
}

/// Decides whether a new status is worth telling Steam about.
#[derive(Debug, Default)]
pub struct WifiFeedPolicy {
    last_pushed: Option<WifiStatus>,
}

impl WifiFeedPolicy {
    /// Returns `true` exactly when `status` differs from what was pushed last.
    pub fn should_push(&mut self, status: &WifiStatus) -> bool {
        if self.last_pushed.as_ref() == Some(status) {
            return false;
        }
        self.last_pushed = Some(status.clone());
        true
    }

    /// Steam lost our state (it restarted or reloaded); push again next time.
    pub fn invalidate(&mut self) {
        self.last_pushed = None;
    }
}

/// The current Wi-Fi connection as the WLAN service reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiReading {
    pub ssid: String,
    /// Link quality, 0-100.
    pub quality: u8,
}

/// Turns readings into what the icon is to show, smoothed, and only when it changes.
#[derive(Debug, Default)]
pub struct IconFeed {
    bars: BarsFilter,
    policy: WifiFeedPolicy,
}

impl IconFeed {
    /// The status to show after `reading` (`None` while not connected), or `None` when the icon
    /// shows it already.
    pub fn next(&mut self, reading: Option<&WifiReading>) -> Option<WifiStatus> {
        let status = if let Some(reading) = reading {
            WifiStatus::Connected {
                ssid: reading.ssid.clone(),
                bars: self.bars.update(reading.quality),
            }
        } else {
            self.bars.reset();
            WifiStatus::Disconnected
        };
        self.policy.should_push(&status).then_some(status)
    }
}

/// The changes a registration with the WLAN service asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiChanges {
    /// Connecting and disconnecting.
    Connection,
    /// Those, and the signal strength, which Windows guards more closely (see `wlan.rs`).
    ConnectionAndSignal,
}

/// Which changes the icon follows, as far as Windows granted them.
#[derive(Debug, PartialEq, Eq)]
pub enum Followed<E> {
    /// Connecting, disconnecting and the signal strength.
    All,
    /// Connecting and disconnecting; the signal strength was refused with `refusal`.
    ConnectionOnly { refusal: E },
    /// None: the last registration tried failed with this.
    Nothing(E),
}

/// Registers for every change that can alter the icon or, where Windows refuses that, for
/// connecting and disconnecting alone. `register` makes one registration; `refused` tells a
/// refusal from any other error, after which nothing more is tried: only the signal strength is
/// guarded more closely, so only a refusal is a reason to ask for less.
pub fn register_changes<E>(
    mut register: impl FnMut(WifiChanges) -> Result<(), E>,
    refused: impl Fn(&E) -> bool,
) -> Followed<E> {
    match register(WifiChanges::ConnectionAndSignal) {
        Ok(()) => Followed::All,
        Err(refusal) if refused(&refusal) => match register(WifiChanges::Connection) {
            Ok(()) => Followed::ConnectionOnly { refusal },
            Err(error) => Followed::Nothing(error),
        },
        Err(error) => Followed::Nothing(error),
    }
}

/// When to register once more after Windows refused the signal strength. It ties that to the
/// location permission, which the first registration may have come before: the query that makes
/// Windows ask is one for a connection, so with Wi-Fi not yet connected nothing asked. The first
/// reading with the connection's details shows the permission is given since; a refusal after
/// such a reading is not about the permission, so it is not tried again.
#[derive(Debug)]
pub struct SignalRetry {
    pending: bool,
}

impl SignalRetry {
    /// After the first registration, which `followed` says the outcome of; `detailed_before`
    /// whether a reading with the connection's details came before it.
    pub fn after_first<E>(followed: &Followed<E>, detailed_before: bool) -> Self {
        Self {
            pending: matches!(followed, Followed::ConnectionOnly { .. }) && !detailed_before,
        }
    }

    /// Whether to register again now, after a reading that had the connection's details or not.
    /// `true` once at most: waits for readings, which come from Windows' notifications, and
    /// never for time.
    pub fn due(&mut self, detailed: bool) -> bool {
        let due = self.pending && detailed;
        self.pending &= !due;
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_buckets() {
        let bars = |q| SignalBars::from_quality(q).get();
        assert_eq!(
            [bars(0), bars(1), bars(25), bars(26), bars(50)],
            [0, 1, 1, 2, 2]
        );
        assert_eq!([bars(51), bars(75), bars(76), bars(100)], [3, 3, 4, 4]);
    }

    #[test]
    fn filter_ignores_jitter_at_a_bucket_edge() {
        let mut filter = BarsFilter::default();
        assert_eq!(filter.update(74).get(), 3);
        assert_eq!(
            filter.update(77).get(),
            3,
            "just over the edge is not enough"
        );
        assert_eq!(filter.update(80).get(), 4);
        assert_eq!(
            filter.update(74).get(),
            4,
            "just under the edge is not enough"
        );
        assert_eq!(filter.update(70).get(), 3);
    }

    #[test]
    fn filter_reports_a_dead_link_immediately() {
        let mut filter = BarsFilter::default();
        assert_eq!(filter.update(2).get(), 1);
        assert_eq!(filter.update(0).get(), 0);
    }

    #[test]
    fn policy_pushes_only_changes() {
        let mut policy = WifiFeedPolicy::default();
        let home = WifiStatus::Connected {
            ssid: "home".to_string(),
            bars: SignalBars::from_quality(80),
        };
        assert!(policy.should_push(&home));
        assert!(!policy.should_push(&home));
        assert!(policy.should_push(&WifiStatus::Disconnected));
        policy.invalidate();
        assert!(policy.should_push(&WifiStatus::Disconnected));
    }

    fn reading(ssid: &str, quality: u8) -> WifiReading {
        WifiReading {
            ssid: ssid.to_string(),
            quality,
        }
    }

    #[test]
    fn the_icon_is_fed_once_per_change() {
        let mut feed = IconFeed::default();
        let shown = feed.next(Some(&reading("home", 80)));
        assert_eq!(
            shown,
            Some(WifiStatus::Connected {
                ssid: "home".to_string(),
                bars: SignalBars::from_quality(80),
            })
        );
        assert_eq!(feed.next(Some(&reading("home", 82))), None, "the same bars");
        assert_eq!(feed.next(None), Some(WifiStatus::Disconnected));
        assert_eq!(feed.next(None), None);
        // A new connection starts the bars afresh: no smoothing against the old one.
        let shown = feed.next(Some(&reading("home", 74)));
        assert!(
            matches!(shown, Some(WifiStatus::Connected { bars, .. }) if bars.get() == 3),
            "{shown:?}"
        );
    }

    /// Windows' codes, as the WLAN reader sees them.
    const ACCESS_DENIED: u32 = 5;
    const RPC_SERVER_UNAVAILABLE: u32 = 1722;

    /// Registers the way `wlan.rs` does against a service that answers each kind of registration
    /// as `answer` says; also what was asked for, in order.
    fn register_with(
        answer: impl Fn(WifiChanges) -> Result<(), u32>,
    ) -> (Followed<u32>, Vec<WifiChanges>) {
        let mut asked = Vec::new();
        let followed = register_changes(
            |changes| {
                asked.push(changes);
                answer(changes)
            },
            |code| *code == ACCESS_DENIED,
        );
        (followed, asked)
    }

    #[test]
    fn with_everything_granted_every_change_is_followed() {
        let (followed, asked) = register_with(|_| Ok(()));
        assert_eq!(followed, Followed::All);
        assert_eq!(asked, [WifiChanges::ConnectionAndSignal]);
    }

    #[test]
    fn a_refused_signal_leaves_the_connection_followed() {
        let (followed, asked) = register_with(|changes| match changes {
            WifiChanges::ConnectionAndSignal => Err(ACCESS_DENIED),
            WifiChanges::Connection => Ok(()),
        });
        assert_eq!(
            followed,
            Followed::ConnectionOnly {
                refusal: ACCESS_DENIED
            }
        );
        assert_eq!(
            asked,
            [WifiChanges::ConnectionAndSignal, WifiChanges::Connection]
        );
    }

    #[test]
    fn with_everything_refused_nothing_is_followed() {
        let (followed, asked) = register_with(|_| Err(ACCESS_DENIED));
        assert_eq!(followed, Followed::Nothing(ACCESS_DENIED));
        assert_eq!(
            asked,
            [WifiChanges::ConnectionAndSignal, WifiChanges::Connection]
        );
    }

    #[test]
    fn a_refusal_before_the_details_is_tried_again_once_at_the_first_detailed_reading() {
        let refused: Followed<u32> = Followed::ConnectionOnly {
            refusal: ACCESS_DENIED,
        };
        let mut retry = SignalRetry::after_first(&refused, false);
        assert!(!retry.due(false), "not connected, or no permission yet");
        assert!(retry.due(true));
        assert!(!retry.due(true), "once only");
    }

    #[test]
    fn a_refusal_after_the_details_or_a_grant_is_not_tried_again() {
        let refused: Followed<u32> = Followed::ConnectionOnly {
            refusal: ACCESS_DENIED,
        };
        let mut after_details = SignalRetry::after_first(&refused, true);
        let mut granted = SignalRetry::after_first(&Followed::<u32>::All, false);
        let mut failed =
            SignalRetry::after_first(&Followed::Nothing(RPC_SERVER_UNAVAILABLE), false);
        for retry in [&mut after_details, &mut granted, &mut failed] {
            assert!(!retry.due(true));
        }
    }

    #[test]
    fn another_error_is_not_tried_again_with_less() {
        let (followed, asked) = register_with(|_| Err(RPC_SERVER_UNAVAILABLE));
        assert_eq!(followed, Followed::Nothing(RPC_SERVER_UNAVAILABLE));
        assert_eq!(asked, [WifiChanges::ConnectionAndSignal]);
    }
}
