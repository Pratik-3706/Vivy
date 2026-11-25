use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PhoneCommand {
    #[serde(rename = "tap")]
    Tap {
        id: String,
        x: f32, // 0.0 to 100.0 (% of screen width)
        y: f32, // 0.0 to 100.0 (% of screen height)
    },
    #[serde(rename = "tap_element")]
    TapElement {
        id: String,
        query: String, // visible text or content description to match
    },
    #[serde(rename = "long_press")]
    LongPress {
        id: String,
        x: f32,
        y: f32,
        duration_ms: u64,
    },
    #[serde(rename = "swipe")]
    Swipe {
        id: String,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        duration_ms: u64,
    },
    #[serde(rename = "type_text")]
    TypeText {
        id: String,
        text: String,
    },
    #[serde(rename = "press_key")]
    PressKey {
        id: String,
        key: String, // "BACK", "HOME", "RECENTS", "ENTER", "NOTIFICATIONS", "VOLUME_UP", "VOLUME_DOWN"
    },
    #[serde(rename = "open_app")]
    OpenApp {
        id: String,
        package_name: String,
        app_name: Option<String>,
    },
    #[serde(rename = "scroll")]
    Scroll {
        id: String,
        direction: String, // "UP", "DOWN", "LEFT", "RIGHT"
        distance: Option<f32>,
    },
    #[serde(rename = "dump_ui")]
    DumpUi {
        id: String,
    },
    #[serde(rename = "start_stream")]
    StartStream {
        id: String,
        fps: u32,
        quality: u32,
        max_dimension: u32,
    },
    #[serde(rename = "stop_stream")]
    StopStream {
        id: String,
    },
    #[serde(rename = "request_frame")]
    RequestFrame {
        id: String,
    },
    #[serde(rename = "get_device_info")]
    GetDeviceInfo {
        id: String,
    },
}

impl PhoneCommand {
    pub fn id(&self) -> &str {
        match self {
            PhoneCommand::Tap { id, .. }
            | PhoneCommand::TapElement { id, .. }
            | PhoneCommand::LongPress { id, .. }
            | PhoneCommand::Swipe { id, .. }
            | PhoneCommand::TypeText { id, .. }
            | PhoneCommand::PressKey { id, .. }
            | PhoneCommand::OpenApp { id, .. }
            | PhoneCommand::Scroll { id, .. }
            | PhoneCommand::DumpUi { id }
            | PhoneCommand::StartStream { id, .. }
            | PhoneCommand::StopStream { id }
            | PhoneCommand::RequestFrame { id }
            | PhoneCommand::GetDeviceInfo { id } => id,
        }
    }
}

fn default_device_name() -> String { "Android Device".to_string() }
fn default_width() -> u32 { 1080 }
fn default_height() -> u32 { 2400 }
fn default_battery() -> i32 { -1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PhoneMessage {
    #[serde(rename = "connected")]
    Connected {
        #[serde(default = "default_device_name")]
        device_name: String,
        #[serde(default)]
        brand: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        android_version: String,
        #[serde(default = "default_width")]
        screen_width: u32,
        #[serde(default = "default_height")]
        screen_height: u32,
        #[serde(default = "default_battery")]
        battery_level: i32,
        #[serde(default)]
        is_charging: bool,
        /// Missing from older phone builds, hence optional.
        #[serde(default)]
        accessibility: Option<bool>,
        #[serde(default)]
        screen_share: Option<bool>,
    },
    /// Sent by the phone whenever accessibility or screen sharing changes.
    #[serde(rename = "device_status")]
    DeviceStatus {
        #[serde(default)]
        accessibility: Option<bool>,
        #[serde(default)]
        screen_share: Option<bool>,
    },
    #[serde(rename = "frame")]
    Frame {
        // In-memory screen share frame ONLY (base64 image).
        // NEVER saved to storage on PC or phone.
        data: String,
        #[serde(default)]
        timestamp: u64,
    },
    #[serde(rename = "command_result")]
    CommandResult {
        id: String,
        success: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        data: Option<String>,
    },
    #[serde(rename = "device_info")]
    DeviceInfo {
        #[serde(default = "default_device_name")]
        device_name: String,
        #[serde(default)]
        brand: String,
        #[serde(default)]
        model: String,
        #[serde(default = "default_width")]
        screen_width: u32,
        #[serde(default = "default_height")]
        screen_height: u32,
        #[serde(default = "default_battery")]
        battery_level: i32,
        #[serde(default)]
        is_charging: bool,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat {
        #[serde(default)]
        timestamp: u64,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
    },
    /// A crash the phone app recorded; sent on the next connection.
    #[serde(rename = "crash_report")]
    CrashReport {
        report: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub connected: bool,
    pub device_name: String,
    pub brand: String,
    pub model: String,
    pub android_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub battery_level: i32,
    pub is_charging: bool,
    pub streaming: bool,
    pub last_seen: u64,
    /// None when the phone app is too old to report it.
    pub accessibility: Option<bool>,
    pub screen_share: Option<bool>,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            connected: false,
            device_name: "No device connected".to_string(),
            brand: String::new(),
            model: String::new(),
            android_version: String::new(),
            screen_width: 1080,
            screen_height: 2400,
            battery_level: -1,
            is_charging: false,
            streaming: false,
            last_seen: 0,
            accessibility: None,
            screen_share: None,
        }
    }
}
