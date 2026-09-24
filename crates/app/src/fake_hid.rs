//! A test device whose button is no key chord but, like a vendor HID report, also reaches the
//! device's own software. It plugs in exactly as a device crate would.

use std::collections::BTreeMap;
use std::os::windows::io::{AsHandle, BorrowedHandle};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mujina_adapter_kit::plugin::{DeviceParts, DevicePlugin, DeviceRuntime, runtime_conformance};
use mujina_application::agent::{AgentEvent, AgentPorts, AgentService, AgentSettings};
use mujina_application::device::{
    self, ButtonId, ButtonSpec, DeviceChoice, DeviceDescriptor, DeviceSelection, Devices,
    Suppression, SystemIdentity,
};
use mujina_application::launcher::{LauncherCaps, LauncherSelection, OptionTable};
use mujina_application::ports::{DeviceButtons, FseState, LauncherState, PortError, PortResult};
use mujina_application::settings::schema::SettingSpec;
use mujina_application::testing::{
    FakeForeground, FakeFse, FakeHomeActivator, FakeInput, FakeLauncher, FakeLauncherDescriptor,
    FakeSettingsSource, device_conformance,
};
use mujina_winutil::event::Event;
use mujina_winutil::wait::{self, EventLoop, WaitSource};

use crate::registry;

struct FakeHid;

static FAKE_HID: FakeHid = FakeHid;

const FAKE_HID_ID: &str = "fake-hid";

impl DeviceDescriptor for FakeHid {
    fn id(&self) -> &str {
        FAKE_HID_ID
    }

    fn name(&self) -> String {
        "Fake HID pad".to_string()
    }

    fn matches(&self, identity: &SystemIdentity) -> bool {
        identity.manufacturer == "Contoso"
    }

    fn buttons(&self) -> Vec<ButtonSpec> {
        vec![ButtonSpec {
            id: ButtonId(1),
            key: "armoury".to_string(),
            label: "Armoury button".to_string(),
            suppression: Suppression::Observed,
        }]
    }

    fn settings(&self) -> &[SettingSpec] {
        &[]
    }
}

/// Signalled by the test; a real device would have an overlapped read signal its own event.
static REPORT: Mutex<Option<Arc<Event>>> = Mutex::new(None);

static PASSED_ON: AtomicU32 = AtomicU32::new(0);

struct FakeHidRuntime;

static FAKE_HID_RUNTIME: FakeHidRuntime = FakeHidRuntime;

impl DeviceRuntime for FakeHidRuntime {
    /// Starts with no id too (the button switched off), as every runtime must.
    fn start(&self, device: &DeviceSelection) -> PortResult<DeviceParts> {
        if let Some(id) = device.id.as_deref()
            && id != FAKE_HID_ID
        {
            return Err(PortError::Failed(format!("{id} is not the fake pad")));
        }
        let report = Arc::new(Event::new().map_err(|error| PortError::Failed(error.to_string()))?);
        *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&report));
        Ok(DeviceParts {
            buttons: Box::new(FakeHidButtons),
            sources: vec![Box::new(ReportSource { report })],
        })
    }
}

struct FakeHidButtons;

impl DeviceButtons for FakeHidButtons {
    fn reconfigure(&self, device: &DeviceSelection) -> bool {
        device.id.as_deref().is_none_or(|id| id == FAKE_HID.id())
    }

    fn pass_on(&self, _button: ButtonId) {
        PASSED_ON.fetch_add(1, Ordering::Relaxed);
    }
}

struct ReportSource {
    report: Arc<Event>,
}

impl WaitSource<AgentEvent> for ReportSource {
    fn name(&self) -> &'static str {
        "fake pad reports"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        Some(self.report.as_handle())
    }

    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::ButtonPressed(ButtonId(1)));
    }
}

static LAUNCHER: FakeLauncherDescriptor =
    FakeLauncherDescriptor::named("fake", "Fake", LauncherCaps::ALL);

/// Mujina's devices, with the pad first, where `registry.rs` puts a device crate.
fn devices() -> Devices {
    let mut all: Vec<&'static dyn DeviceDescriptor> = vec![&FAKE_HID];
    all.extend(registry::devices().all);
    Devices {
        all: Box::leak(all.into_boxed_slice()),
        own: registry::devices().own,
    }
}

/// Signals the pad's report and hands the first event to an agent with `fse` and `in_front`.
fn press(
    device_parts: DeviceParts,
    device: &DeviceSelection,
    fse: FseState,
    in_front: &str,
) -> FakeInput {
    let fse = FakeFse(fse);
    let foreground = FakeForeground::default();
    foreground.set(Some(in_front));
    let launcher = FakeLauncher::installed(LauncherState::UiVisible);
    let keys = FakeInput::default();
    let home = FakeHomeActivator::default();
    let settings = FakeSettingsSource::default();
    let ports = AgentPorts {
        fse: &fse,
        foreground: &foreground,
        launcher: &launcher,
        descriptor: &LAUNCHER,
        buttons: device_parts.buttons.as_ref(),
        devices: devices(),
        keys: &keys,
        home: &home,
        settings: &settings,
    };
    let started = AgentSettings {
        standalone: true,
        device: device.clone(),
        launcher: LauncherSelection {
            id: LAUNCHER.id.to_string(),
            options: OptionTable::new(),
        },
        ..AgentSettings::default()
    };
    let mut agent = AgentService::new(ports, started);
    agent.start();

    let mut events = EventLoop::new();
    for source in device_parts.sources {
        events.add(source).unwrap();
    }
    // A loop that never wakes ends the test instead of hanging it.
    events.wake_at(
        Instant::now() + Duration::from_secs(10),
        AgentEvent::SessionEnding,
    );
    let report = REPORT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap();
    report.set();
    let mut seen = Vec::new();
    events
        .run(&mut |_| {}, &mut |event| {
            agent.handle(&event);
            seen.push(event);
            wait::Flow::Exit
        })
        .unwrap();
    assert_eq!(seen, [AgentEvent::ButtonPressed(ButtonId(1))]);
    assert_eq!(home.activations.get(), 0);
    keys
}

#[test]
fn a_device_with_a_mechanism_of_its_own_plugs_in_without_touching_the_core() {
    let plugin = DevicePlugin {
        descriptor: &FAKE_HID,
        runtime: &FAKE_HID_RUNTIME,
    };
    device_conformance(plugin.descriptor);

    let contoso = SystemIdentity {
        manufacturer: "Contoso".to_string(),
        product: "Pad".to_string(),
    };
    let mut notes = Vec::new();
    let device = device::select(
        &devices(),
        &contoso,
        &DeviceChoice::Auto,
        &OptionTable::new(),
        &BTreeMap::new(),
        &mut notes,
    );
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(device.id.as_deref(), Some("fake-hid"));

    let parts = plugin.runtime.start(&device).unwrap();
    let keys = press(parts, &device, FseState::Active, "fakelauncher.exe");
    assert_eq!(keys.sent(), [FakeLauncher::MENU]);

    // The pad's own software had the press anyway, so nothing is passed on.
    let parts = plugin.runtime.start(&device).unwrap();
    let keys = press(parts, &device, FseState::Inactive, "explorer.exe");
    assert!(keys.sent().is_empty());
    assert_eq!(PASSED_ON.load(Ordering::Relaxed), 0);

    runtime_conformance(plugin.runtime, &device).unwrap();
    let other = DeviceSelection {
        id: Some("onexplayer".to_string()),
        ..DeviceSelection::none()
    };
    assert!(plugin.runtime.start(&other).is_err());
}
