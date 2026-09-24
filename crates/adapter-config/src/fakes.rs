//! Launchers and devices for the tests, like the real ones in adapters this crate cannot use.

use mujina_application::device::Devices;
use mujina_application::launcher::{LauncherCaps, Launchers};
use mujina_application::settings::schema::{Applies, SettingKind, SettingSpec, TextFormat};
use mujina_application::testing::{FakeDevice, FakeLauncherDescriptor};

pub static STEAM: FakeLauncherDescriptor = FakeLauncherDescriptor {
    settings: &[
        SettingSpec {
            key: "ui_link",
            kind: SettingKind::Toggle { default: true },
            title: "UI link",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "wifi_indicator",
            kind: SettingKind::Toggle { default: true },
            title: "Wi-Fi icon",
            help: "",
            applies: Applies::NextSession,
            requires: Some("ui_link"),
            required: false,
        },
    ],
    template: "[launcher.steam]\n\
               # Steam, kind = \"steam\" (the default).\n\
               # ui_link = true            # the debugging port\n\
               # wifi_indicator = true     # the Wi-Fi icon; needs ui_link\n",
    ..FakeLauncherDescriptor::named("steam", "Steam Big Picture", LauncherCaps::ALL)
};

pub static GENERIC: FakeLauncherDescriptor = FakeLauncherDescriptor {
    settings: &[
        SettingSpec {
            key: "executable",
            kind: SettingKind::Text {
                format: TextFormat::Path,
            },
            title: "Program",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: true,
        },
        SettingSpec {
            key: "arguments",
            kind: SettingKind::TextList,
            title: "Start it with",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "window_class",
            kind: SettingKind::Text {
                format: TextFormat::Plain,
            },
            title: "Window class",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "process",
            kind: SettingKind::Text {
                format: TextFormat::Plain,
            },
            title: "Process name",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
    ],
    template: "# Another launcher, with kind = \"generic\" above:\n\
               # [launcher.generic]\n\
               # executable = 'C:\\Path\\To\\Frontend.exe'\n\
               # arguments = [\"--fullscreen\"]\n\
               # window_class = \"...\"\n\
               # process = \"frontend.exe\"\n",
    refuses: Some(("executable", "C:\\")),
    ..FakeLauncherDescriptor::named(
        "generic",
        "Another launcher",
        LauncherCaps {
            game_detection: false,
            menu: true,
            overlay: false,
            navigation: false,
        },
    )
};

pub static LAUNCHERS: Launchers = Launchers {
    all: &[&STEAM, &GENERIC],
    fallback: &STEAM,
};

pub static ONEXPLAYER: FakeDevice = FakeDevice {
    manufacturer: "ONE-NETBOOK",
    ..FakeDevice::named("onexplayer", "OneXPlayer (show desktop button)")
};

pub static PAD: FakeDevice = FakeDevice {
    settings: &[SettingSpec {
        key: "mode",
        kind: SettingKind::Toggle { default: false },
        title: "Mode",
        help: "",
        applies: Applies::Live,
        requires: None,
        required: false,
    }],
    ..FakeDevice::named("pad", "A pad")
};

pub static OWN: FakeDevice = FakeDevice {
    needs_both: Some(("modifier", "key")),
    ..FakeDevice::named("custom", "Your own button")
};

pub static DEVICES: Devices = Devices {
    all: &[&ONEXPLAYER, &PAD, &OWN],
    own: &OWN,
};

#[test]
fn the_fakes_keep_the_rules_of_the_real_ones() {
    mujina_application::testing::conformance(&STEAM);
    mujina_application::testing::conformance(&GENERIC);
    for device in DEVICES.all {
        mujina_application::testing::device_conformance(*device);
    }
}
