# Vivy Setup & Build Guide

This guide walks you through setting up, building, and running both the **PC Desktop Application** and the **Android Companion App**.

---

## 1. Prerequisites

### For PC (Desktop App)
- **Rust Toolchain**: `rustc` & `cargo` 1.75+ (run `cargo --version`)
- **Node.js**: v18+ & `npm`
- **Tauri CLI**: Installed automatically via npm dependencies

### For Android (Phone App)
- **Android Studio** (Hedgehog, Iguana, Koala, or later)
- **Android device**: Android 5.0 (API 21) or later. Full tap and swipe control requires Android 7.0 (API 24) or later.
- **Kotlin**: 1.9+

---

## 2. PC Desktop Application Setup

### Development Mode
From the project root:
```powershell
cd desktop
npm install
npm run tauri dev
```
This boots the React dashboard with hot reloading and starts the embedded Rust agent backend and WebSocket server on port `8765`.

### Building the Production Single `.exe`
```powershell
cd desktop
npm run tauri build
```
The compiled standalone binary `Vivy.exe` and installer will be located in:
`desktop/src-tauri/target/release/`

### Automated Build & Release to `releases/` Folder
You can also run the automated build script to compile both PC `.exe` and Android `.apk` and automatically copy them to the git-ignored `releases/` directory:
```powershell
powershell -ExecutionPolicy Bypass -File scripts/build_and_package.ps1
```
Output binaries:
- `releases/pc/Vivy.exe`
- `releases/android/VivyAgent.apk`
*(The `releases/` folder is pre-configured in `.gitignore` so binaries are never committed to your git repository).*

### Configuring API Keys in the PC App
1. Launch Vivy on your PC.
2. Click **Settings** in the top bar.
3. Choose a **provider**: AICredits, OpenAI, Anthropic, Google Gemini, or **Custom** (any OpenAI-compatible endpoint, such as OpenRouter or a local server).
4. Paste that provider's **API key**. Each provider keeps its own key, so you can switch back and forth.
5. Pick a **model**. The list suggests mid-tier models that accept images and support tool calling; you can also type any model ID.
6. Optionally enter a **Tavily** key to enable web search.
7. Click **Save**.
   - Keys are stored in **Windows Credential Manager**, not in plain-text files.

---

## 3. Android Phone App Setup

### Building and Installing with Android Studio
1. Launch **Android Studio**.
2. Click **Open** and select the `vivy/android` folder.
3. Wait for Gradle sync to complete.
4. Connect your Android device via USB (or Wi-Fi debugging).
5. Click the green **Run (▶)** button in Android Studio to build and install `Vivy Agent` onto your phone.

### Alternatively via Command Line:
```powershell
cd android
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

---

## 4. Pairing and Usage

1. **Verify Same Wi-Fi Network**: Ensure your PC and Android phone are connected to the same Wi-Fi or local LAN router.
2. **Launch PC App**: The status bar displays your PC's local pairing address (e.g., `ws://192.168.1.105:8765`).
3. **Launch Android App**:
   - Tap **Enable** next to **Accessibility Service** -> Turn ON `Vivy Agent` in your Android system accessibility settings.
   - Tap **Grant** next to **Live Screen Share** -> Allow system MediaProjection prompt (note: zero video files are saved to disk).
   - Enter your PC IP address and port `8765`.
   - Tap **Connect to PC**.
4. **Control & Automate**:
   - Your phone screen appears live in the PC dashboard.
   - You can click/swipe directly on the mirrored screen with your mouse.
   - Enter prompts in the AI chat box (e.g. *"Open YouTube and search for tech podcasts"*).
   - Watch Vivy visually ground buttons, calculate coordinates, and execute gestures!
