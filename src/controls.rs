//! Quick controls: what is playing, Wi-Fi, Bluetooth, VPN, airplane mode, hotspot and volume.
use crate::system::{open, wstr};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::*;
use windows::Devices::Radios::*;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;

/// What the worker last saw. Read it with `snapshot`.
#[derive(Clone, Default)]
pub struct State {
    pub media: Option<Media>,
    pub art: Option<Arc<Vec<u8>>>, // cover of the current track, encoded
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub vpn: Option<bool>,
    pub airplane: Option<bool>,
    pub hotspot: Option<bool>,
}

static STATE: Mutex<State> = Mutex::new(State {
    media: None,
    art: None,
    wifi: None,
    bluetooth: None,
    vpn: None,
    airplane: None,
    hotspot: None,
});

pub fn snapshot() -> State {
    STATE.lock().unwrap().clone()
}

#[derive(Clone, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub art_id: u64, // goes up by one when the cover changes
}

pub enum Cmd {
    Prev,
    Toggle,
    Next,
    Wifi,
    Bluetooth,
    Vpn,
    Airplane,
    Hotspot,
}

/// WinRT work on an MTA thread: the system media session, which Spotify, browsers and anything
/// else using SMTC report to, plus the radios, airplane mode and the mobile hotspot.
pub fn spawn_winrt(hwnd: HWND) -> Sender<Cmd> {
    let (tx, rx) = channel();
    let h = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        // Windows N editions ship without the media session until the Media Feature Pack is
        // installed, so keep asking, but never let that hold up the radios or VPN
        let mut mgr = None;
        let _ = Radio::RequestAccessAsync().and_then(|a| a.get());
        let (mut last, mut art_key, mut art_id) = (None::<Media>, String::new(), 0u64);
        let mut last_radios = (None, None, None, None, None);
        loop {
            let cmd = rx.recv_timeout(Duration::from_millis(1000));
            if let Err(RecvTimeoutError::Disconnected) = cmd {
                return;
            }
            if mgr.is_none() {
                mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                    .and_then(|a| a.get())
                    .ok();
            }
            let session = mgr.as_ref().and_then(|m| m.GetCurrentSession().ok());
            match (&cmd, &session) {
                (Ok(Cmd::Wifi), _) => toggle_radio(RadioKind::WiFi),
                (Ok(Cmd::Bluetooth), _) => toggle_radio(RadioKind::Bluetooth),
                (Ok(Cmd::Vpn), _) => toggle_vpn(),
                (Ok(Cmd::Airplane), _) => toggle_airplane(),
                (Ok(Cmd::Hotspot), _) => toggle_hotspot(),
                (Ok(c), Some(s)) => {
                    let r = match c {
                        Cmd::Prev => s.TrySkipPreviousAsync(),
                        Cmd::Next => s.TrySkipNextAsync(),
                        _ => s.TryTogglePlayPauseAsync(),
                    };
                    let _ = r.and_then(|a| a.get());
                }
                _ => {}
            }
            let radios = (
                radio_on(RadioKind::WiFi),
                radio_on(RadioKind::Bluetooth),
                Some(vpn_on()),
                airplane_on(),
                hotspot_on(),
            );
            let mut art = None;
            let media =
                session.and_then(|s| read_media(&s, &mut art_key, &mut art_id, &mut art).ok());
            let media_changed = media.as_ref().map(|m| (&m.title, m.playing, m.art_id))
                != last.as_ref().map(|m| (&m.title, m.playing, m.art_id));
            let changed = media_changed || radios != last_radios || cmd.is_ok();
            {
                let mut sh = STATE.lock().unwrap();
                if let Some(a) = art {
                    sh.art = a;
                }
                if media.is_none() {
                    sh.art = None;
                }
                sh.media = media.clone();
                (sh.wifi, sh.bluetooth, sh.vpn, sh.airplane, sh.hotspot) = radios;
            }
            (last, last_radios) = (media, radios);
            if changed {
                crate::system::notify(h);
            }
        }
    });
    tx
}

/// Only touches `art` when the track changed. It then holds the new cover, or None if the track has none.
fn read_media(
    s: &GlobalSystemMediaTransportControlsSession,
    art_key: &mut String,
    art_id: &mut u64,
    art: &mut Option<Option<Arc<Vec<u8>>>>,
) -> Result<Media> {
    let p = s.TryGetMediaPropertiesAsync()?.get()?;
    let (title, artist) = (p.Title()?.to_string(), p.Artist()?.to_string());
    let playing = s.GetPlaybackInfo()?.PlaybackStatus()?
        == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
    let key = format!("{title}\u{1}{artist}");
    if key != *art_key {
        *art_key = key;
        *art_id += 1;
        *art = Some(
            p.Thumbnail()
                .ok()
                .and_then(|t| read_stream(&t).ok())
                .map(Arc::new),
        );
    }
    Ok(Media {
        title,
        artist,
        playing,
        art_id: *art_id,
    })
}

fn read_stream(t: &windows::Storage::Streams::IRandomAccessStreamReference) -> Result<Vec<u8>> {
    let stream = t.OpenReadAsync()?.get()?;
    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream)?;
    reader.LoadAsync(size)?.get()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(buf)
}

fn radios() -> Vec<Radio> {
    Radio::GetRadiosAsync()
        .and_then(|a| a.get())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn radio_on(kind: RadioKind) -> Option<bool> {
    radios()
        .into_iter()
        .find(|r| r.Kind().ok() == Some(kind))
        .and_then(|r| r.State().ok())
        .map(|s| s == RadioState::On)
}

fn toggle_radio(kind: RadioKind) {
    for r in radios().into_iter().filter(|r| r.Kind().ok() == Some(kind)) {
        let next = if r.State().ok() == Some(RadioState::On) {
            RadioState::Off
        } else {
            RadioState::On
        };
        let _ = r.SetStateAsync(next).and_then(|a| a.get());
    }
}

/// Windows' own switch for airplane mode, the one its Settings app uses. Microsoft doesn't
/// document it; the layout comes from people who took RMApi.dll apart.
#[allow(non_snake_case)]
mod rmapi {
    use windows::core::{IUnknown, IUnknown_Vtbl, GUID, HRESULT};
    use windows::Win32::System::Com::*;

    #[windows_core::interface("db3afbfb-08e6-46c6-aa70-bf9a34c30ab7")]
    unsafe trait IRadioManager: IUnknown {
        fn IsRMSupported(&self, state: *mut u32) -> HRESULT;
        fn GetUIRadioInstances(&self, instances: *mut *mut std::ffi::c_void) -> HRESULT;
        /// `radios_on` is 0 in airplane mode. The other two must be valid pointers.
        fn GetSystemRadioState(
            &self,
            radios_on: *mut i32,
            p2: *mut i32,
            reason: *mut u32,
        ) -> HRESULT;
        fn SetSystemRadioState(&self, radios_on: i32) -> HRESULT;
    }

    const RADIO_MANAGEMENT_API: GUID = GUID::from_u128(0x581333f6_28db_41be_bc7a_ff201f12f3f6);

    fn radio_manager() -> Option<IRadioManager> {
        unsafe { CoCreateInstance(&RADIO_MANAGEMENT_API, None, CLSCTX_LOCAL_SERVER).ok() }
    }

    pub fn airplane_on() -> Option<bool> {
        let (mut on, mut p2, mut reason) = (0, 0, 0);
        unsafe {
            radio_manager()?
                .GetSystemRadioState(&mut on, &mut p2, &mut reason)
                .ok()
                .ok()?;
        }
        Some(on == 0)
    }

    /// False when there's no switch to set.
    pub fn set_airplane(on: bool) -> bool {
        radio_manager().is_some_and(|rm| unsafe { rm.SetSystemRadioState(i32::from(!on)).is_ok() })
    }
}
use rmapi::airplane_on;

/// Without the switch (an edition that lacks it, say) this opens airplane mode in Settings.
fn toggle_airplane() {
    if !airplane_on().is_some_and(|on| rmapi::set_airplane(!on)) {
        open("ms-settings:network-airplanemode");
    }
}

/// The mobile hotspot that shares this PC's internet connection.
fn tethering() -> Option<windows::Networking::NetworkOperators::NetworkOperatorTetheringManager> {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::NetworkOperatorTetheringManager;
    let profile = NetworkInformation::GetInternetConnectionProfile().ok()?;
    NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).ok()
}

/// None while offline: there's no connection to share.
fn hotspot_on() -> Option<bool> {
    use windows::Networking::NetworkOperators::TetheringOperationalState;
    let state = tethering()?.TetheringOperationalState().ok()?;
    Some(state == TetheringOperationalState::On)
}

fn toggle_hotspot() {
    let Some(t) = tethering() else {
        return open("ms-settings:network-mobilehotspot");
    };
    let r = if hotspot_on() == Some(true) {
        t.StopTetheringAsync()
    } else {
        t.StartTetheringAsync()
    };
    let _ = r.and_then(|a| a.get());
}

const ERROR_BUFFER_TOO_SMALL: u32 = 603;

/// Calls a RAS enumerator, growing the buffer once if it says it needs more.
fn ras_list<T: Clone>(blank: T, call: impl Fn(&mut [T], &mut u32, &mut u32) -> u32) -> Vec<T> {
    let size = std::mem::size_of::<T>();
    let mut buf = vec![blank.clone(); 8];
    let (mut cb, mut n) = ((buf.len() * size) as u32, 0u32);
    let mut r = call(&mut buf, &mut cb, &mut n);
    if r == ERROR_BUFFER_TOO_SMALL {
        buf = vec![blank; (cb as usize).div_ceil(size)];
        r = call(&mut buf, &mut cb, &mut n);
    }
    if r != 0 {
        return vec![];
    }
    buf.truncate(n as usize);
    buf
}

fn ras_connections() -> Vec<windows::Win32::NetworkManagement::Rras::RASCONNW> {
    use windows::Win32::NetworkManagement::Rras::*;
    let blank = RASCONNW {
        dwSize: std::mem::size_of::<RASCONNW>() as u32,
        ..Default::default()
    };
    ras_list(blank, |b, cb, n| unsafe {
        RasEnumConnectionsW(Some(b.as_mut_ptr()), cb, n)
    })
}

/// VPN profiles set up in Windows Settings.
fn ras_entries() -> Vec<String> {
    use windows::Win32::NetworkManagement::Rras::*;
    let blank = RASENTRYNAMEW {
        dwSize: std::mem::size_of::<RASENTRYNAMEW>() as u32,
        ..Default::default()
    };
    ras_list(blank, |b, cb, n| unsafe {
        RasEnumEntriesW(None, None, Some(b.as_mut_ptr()), cb, n)
    })
    .iter()
    .map(|e| wstr(&e.szEntryName))
    .collect()
}

/// A Windows VPN is connected, or a VPN app's network adapter is up.
/// ponytail: VPN apps are recognised by adapter name, so an unusual one can be missed.
fn vpn_on() -> bool {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    if !ras_connections().is_empty() {
        return true;
    }
    const NAMES: [&str; 9] = [
        "wireguard",
        "tailscale",
        "tap-windows",
        "openvpn",
        "nordlynx",
        "wintun",
        "zerotier",
        "vpn",
        "fortinet",
    ];
    let mut buf = vec![0u64; 4096]; // u64 keeps the adapter structs aligned
    let mut size = (buf.len() * 8) as u32;
    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let call = |buf: &mut Vec<u64>, size: &mut u32| {
            GetAdaptersAddresses(
                0,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                size,
            )
        };
        let mut r = call(&mut buf, &mut size);
        if r == ERROR_BUFFER_OVERFLOW.0 {
            // machines with Hyper-V, WSL or Docker have many adapters
            buf = vec![0u64; (size as usize).div_ceil(8)];
            r = call(&mut buf, &mut size);
        }
        if r != 0 {
            return false;
        }
        let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while let Some(ad) = a.as_ref() {
            let desc = ad
                .Description
                .to_string()
                .unwrap_or_default()
                .to_lowercase();
            if ad.OperStatus == IfOperStatusUp
                && (ad.IfType == 23 /* PPP */ || NAMES.iter().any(|n| desc.contains(n)))
            {
                return true;
            }
            a = ad.Next;
        }
    }
    false
}

/// Disconnects Windows VPNs, or connects the first profile with its saved sign-in. VPN apps
/// can't be switched from outside, so with no Windows profile this opens VPN settings.
fn toggle_vpn() {
    let conns = ras_connections();
    if !conns.is_empty() {
        for c in conns {
            unsafe {
                windows::Win32::NetworkManagement::Rras::RasHangUpW(c.hrasconn);
            }
        }
        return;
    }
    match ras_entries().first() {
        Some(name) => {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("rasdial.exe")
                .arg(name)
                .creation_flags(0x0800_0000)
                .status();
        }
        None => open("ms-settings:network-vpn"),
    }
}

pub struct Volume {
    pub level: f32,
    pub muted: bool,
    pub name: String,
}

fn devices() -> Option<windows::Win32::Media::Audio::IMMDeviceEnumerator> {
    use windows::Win32::Media::Audio::*;
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

fn endpoint() -> Option<(
    windows::Win32::Media::Audio::IMMDevice,
    windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
)> {
    use windows::Win32::Media::Audio::*;
    unsafe {
        let dev = devices()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let vol = dev.Activate(CLSCTX_ALL, None).ok()?;
        Some((dev, vol))
    }
}

/// Whether the microphone calls use (the default communications device) is muted.
/// None without a microphone.
pub fn mic_muted() -> Option<bool> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::*;
    unsafe {
        let dev = devices()?
            .GetDefaultAudioEndpoint(eCapture, eCommunications)
            .ok()?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        Some(vol.GetMute().ok()?.as_bool())
    }
}

/// Mutes or unmutes every microphone, so apps that picked another one are muted too.
pub fn toggle_mic() {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::*;
    let mute = !mic_muted().unwrap_or(true);
    let Some(list) =
        devices().and_then(|e| unsafe { e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE).ok() })
    else {
        return;
    };
    unsafe {
        for i in 0..list.GetCount().unwrap_or(0) {
            let vol = list
                .Item(i)
                .and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None));
            if let Ok(v) = vol {
                let _ = v.SetMute(mute, std::ptr::null());
            }
        }
    }
}

impl Volume {
    /// The device's short name, or "Volume" when it has none.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            "Volume"
        } else {
            &self.name
        }
    }
}

/// Default output device: level, mute, and its short name ("Speakers", "Headphones").
pub fn volume() -> Option<Volume> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    let (dev, vol) = endpoint()?;
    unsafe {
        let name = dev
            .OpenPropertyStore(STGM_READ)
            .and_then(|p| p.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_default();
        let name = name.split(" (").next().unwrap_or("").to_string();
        Some(Volume {
            level: vol.GetMasterVolumeLevelScalar().ok()?,
            muted: vol.GetMute().ok()?.as_bool(),
            name,
        })
    }
}

/// Sets the output level, and unmutes.
pub fn set_volume(level: f32) {
    let Some((_, vol)) = endpoint() else { return };
    unsafe {
        let _ = vol.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null());
        let _ = vol.SetMute(false, std::ptr::null());
    }
}

pub fn toggle_mute() {
    let Some((_, vol)) = endpoint() else { return };
    unsafe {
        let muted = vol.GetMute().map(|m| m.as_bool()).unwrap_or(false);
        let _ = vol.SetMute(!muted, std::ptr::null());
    }
}
