# Vivy WebSocket Communication Protocol (PC ↔ Android)

Vivy uses a low-latency, bidirectional WebSocket connection over TLS (`wss://<PC_LAN_IP>:8765`) to stream live in-memory screen mirror frames and execute autonomous navigation commands.

---

## 🔐 Pairing and authentication

The PC uses a self-signed certificate it creates on first run (`%LOCALAPPDATA%\Vivy\tls`). Its fingerprint is the SHA-256 of the certificate (DER), as lowercase hex. The first message on every connection must be `auth` or `pair_hello`; anything else closes the connection, and no frames or commands are exchanged until the PC accepts the phone.

**Pairing** (the PC shows a 12-character code, valid for 5 minutes):

```
phone → {"type":"pair_hello","nonce":"<phone nonce, 32 hex>"}
PC    → {"type":"pair_challenge","nonce":"<pc nonce>","proof":"<PC proof>"}
phone → {"type":"pair_confirm","proof":"<phone proof>","device_name":"Pixel 8"}
PC    → {"type":"paired","token":"<64 hex>","pc_name":"DESKTOP-1"}
```

`proof = hex(HMAC-SHA256(key = code, msg = "vivy-pair|<role>|<fingerprint>|<phone nonce>|<pc nonce>"))`, with role `pc` or `phone`. The code is normalized first: uppercase, separators removed, `O`→`0`, `I`/`L`→`1`. The phone checks the PC's proof against the fingerprint of the certificate it actually received before sending its own proof. It then pins that fingerprint and stores the token. Errors come back as `{"type":"pair_error","message":"..."}`.

**Reconnecting**: the phone accepts only the pinned certificate and sends `{"type":"auth","token":"..."}`. The PC answers `{"type":"auth_ok","pc_name":"..."}` or `{"type":"auth_failed","message":"..."}`. The PC stores only the token's SHA-256.

---

## 🔒 Privacy & Zero-Recording Guarantee

1. **In-Memory Streaming Only**:
   - Frames transmitted by the phone are received directly into volatile memory (RAM).
   - Neither the Android app nor the PC desktop app saves any screen recording, video file, or frame cache to physical disk.
2. **Permission Guard**:
   - Any access to personal items (photos, storage files, chat messages, email messages) or destructive actions (deletions) triggers an interactive authorization modal on the PC dashboard before execution.

---

## 📐 Normalized Screen Coordinates

All touch, swipe, and gesture commands operate on **normalized percentages from top-left (0.0% to 100.0%)**:
- `(0.0, 0.0)` is top-left of the display.
- `(100.0, 100.0)` is bottom-right of the display.
- The Android Accessibility Service converts these percentages to exact screen resolution pixels on the fly, ensuring cross-device consistency regardless of aspect ratio or display resolution.

---

## 📱 Phone → PC (Messages)

### 1. Connected Handshake (`connected`)
Sent immediately when WebSocket connection is established:
```json
{
  "type": "connected",
  "device_name": "Google Pixel 8 Pro",
  "brand": "Google",
  "model": "Pixel 8 Pro",
  "android_version": "14",
  "screen_width": 1080,
  "screen_height": 2400,
  "battery_level": 88,
  "is_charging": false
}
```

### 2. Live In-Memory Screen Frame
Sent as a **binary WebSocket message containing raw JPEG bytes** (about a third smaller than base64 JSON), at up to the `fps` requested by `start_stream`. New frames are only sent when the screen changes.

The PC still accepts the older JSON form:
```json
{
  "type": "frame",
  "data": "<base64_encoded_jpeg_or_webp_image>",
  "timestamp": 1728345678901
}
```

### 3. Command Execution Result (`command_result`)
Returned after dispatching gestures:
```json
{
  "type": "command_result",
  "id": "c1f73b64-...",
  "success": true,
  "error": null,
  "data": null
}
```
`data` carries a text payload for commands that return one (`dump_ui`, `tap_element`, `open_app`).

> **Every phone message must include `type`.** On Android, `kotlinx.serialization` drops properties that equal their default unless `encodeDefaults = true`, and `type` is declared as a default. Without that flag the PC can't parse anything the phone sends.

### 4. Heartbeat (`heartbeat`)
```json
{
  "type": "heartbeat",
  "timestamp": 1728345678901
}
```

---

## 💻 PC → Phone (Commands)

### 1. Tap (`tap`)
```json
{
  "id": "uuid-v4",
  "type": "tap",
  "x": 48.5,
  "y": 62.0
}
```

### 2. Long Press (`long_press`)
```json
{
  "id": "uuid-v4",
  "type": "long_press",
  "x": 50.0,
  "y": 50.0,
  "duration_ms": 800
}
```

### 3. Swipe (`swipe`)
```json
{
  "id": "uuid-v4",
  "type": "swipe",
  "x1": 50.0,
  "y1": 80.0,
  "x2": 50.0,
  "y2": 20.0,
  "duration_ms": 300
}
```

### 4. Scroll (`scroll`)
```json
{
  "id": "uuid-v4",
  "type": "scroll",
  "direction": "DOWN"
}
```

### 5. Type Text (`type_text`)
```json
{
  "id": "uuid-v4",
  "type": "type_text",
  "text": "Lo-Fi relaxing music"
}
```

### 6. Press System Navigation Key (`press_key`)
```json
{
  "id": "uuid-v4",
  "type": "press_key",
  "key": "BACK"
}
```
Supported keys: `BACK`, `HOME`, `RECENTS`, `NOTIFICATIONS`, `QUICK_SETTINGS`, `ENTER` (Android 11+, submits the focused field), `VOLUME_UP`, `VOLUME_DOWN`.

### 7. Launch Installed App (`open_app`)
```json
{
  "id": "uuid-v4",
  "type": "open_app",
  "package_name": "",
  "app_name": "YouTube"
}
```
The package is tried first; otherwise launcher apps are matched by label. Returns `data` like `Opened YouTube (com.google.android.youtube)`.

### Tap by Text (`tap_element`)
```json
{ "id": "uuid-v4", "type": "tap_element", "query": "Search" }
```
Clicks the visible element whose text or content description best matches `query`. On failure, `error` lists the labels that are visible.

### Read the Screen (`dump_ui`)
```json
{ "id": "uuid-v4", "type": "dump_ui" }
```
Returns `data` with the foreground app and one line per visible element:
```
Foreground app: com.google.android.youtube
- "Search" ImageButton [tap] @ (84.2, 6.1)
```

### 8. Stream Controls (`start_stream` / `stop_stream`)
```json
{
  "type": "start_stream",
  "fps": 12,
  "quality": 70,
  "max_dimension": 1280
}
```
