<p align="center">
  <img src="assets/vivy.png" alt="Vivy" width="96" />
</p>

<h1 align="center">Vivy</h1>

<p align="center"><b>An AI that uses your Android phone for you.</b><br/>
Tell Vivy what you want in plain words. She plans it, her helper Matsumoto taps through it, and you watch it happen live on your PC, or from a terminal on Windows and Linux.</p>

<p align="center">
  <img alt="Desktop app" src="https://img.shields.io/badge/desktop-Windows-2b2b2b" />
  <img alt="Terminal app" src="https://img.shields.io/badge/terminal-Windows%20%C2%B7%20Linux-2b2b2b" />
  <img alt="Phone" src="https://img.shields.io/badge/phone-Android%207%2B-2b2b2b" />
  <img alt="Built with" src="https://img.shields.io/badge/built%20with-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20React%20%C2%B7%20Kotlin-2b2b2b" />
  <img alt="License" src="https://img.shields.io/badge/license-MIT-2b2b2b" />
</p>

---

## What it does

Vivy runs the AI on your computer and the hands on your phone:

- **Vivy for PC** (`Vivy.exe`): a desktop app with a chat, a live mirror of your phone screen, and the AI agent itself.
- **Vivy in the terminal** (`vivy`): the same agent as a command-line app for **Windows and Linux**, with an animated pixel-art Matsumoto. See [Vivy in the terminal](#vivy-in-the-terminal).
- **Vivy Agent for Android**: a small app that streams your screen to the computer and carries out taps, swipes and typing through Android's Accessibility service. No root, no USB cable, no `adb`.

You type something like *"Open YouTube and search for lo-fi music"*. Vivy reads the screen, decides what to do, and Matsumoto (the little cube) performs each step on the phone: opening apps, tapping buttons by their label, scrolling, typing, and checking the result before moving on.

### Highlights

- **Wireless and rootless.** Phone and PC talk over your local Wi-Fi.
- **Live screen mirror.** Frames stream in memory only; nothing is recorded or saved.
- **Reads the screen as text first.** It uses the accessibility tree to tap elements by name, and falls back to looking at pixels only for games, maps and other canvas-drawn apps.
- **You stay in control.**
  - A global **Stop** hotkey (`Ctrl+Shift+X`) works even when the window isn't focused.
  - **Pause** lets you use the phone yourself, then resume.
  - An agent on/off switch.
  - Permission prompts before sensitive or destructive actions.
- **Live thinking.** Reasoning and replies stream in as the model writes them.
- **Chats with memory.** Each chat remembers what was said and done. Long chats are summarized automatically so they stay within the model's limits.
- **Resend, retry and edit** any earlier message. Everything after it is replaced.
- **Pick your provider**: AICredits, OpenAI, Anthropic, Google Gemini, or any OpenAI-compatible endpoint.
- **Keys stay local.** They are stored in the system keyring (Windows Credential Manager, or the Secret Service on Linux).

---

## Supported providers

All providers are used through the OpenAI-compatible chat-completions API. The suggested models are mostly mid-tier: strong enough for multi-step phone tasks, without flagship prices. Each provider also lists its top model for complex tasks. All of them accept images and support tool calling.

| Provider | Everyday models | For complex tasks | Get a key |
|---|---|---|---|
| **AICredits** (default, one key for many models) | `z-ai/glm-5.3-flash`, `anthropic/claude-haiku-5.5`, `openai/gpt-6-luna`, `deepseek/deepseek-v4.1-flash`, `moonshotai/kimi-k2.6` | `anthropic/claude-opus-5.5` | [aicredits.in](https://aicredits.in) |
| **OpenAI** | `gpt-6.1-sol`, `gpt-6-luna`, `gpt-5-mini` | `gpt-6-astra` | [platform.openai.com](https://platform.openai.com/api-keys) |
| **Anthropic** | `claude-sonnet-5-5`, `claude-sonnet-5`, `claude-haiku-5-5` | `claude-opus-5-5` | [console.anthropic.com](https://console.anthropic.com/settings/keys) |
| **Google Gemini** | `gemini-3.8-flash`, `gemini-3.6-flash`, `gemini-3.5-flash-lite` | `gemini-4-argon` | [aistudio.google.com](https://aistudio.google.com/apikey) |
| **Custom** | Any model ID | | OpenRouter, a local server, or any OpenAI-compatible URL |

You can type any other model ID in Settings. It just needs to accept images and support tool calling.

**Thinking.** Vivy shows the model's reasoning live when the provider sends it, and lets you set a thinking effort (Default, Low, Medium, High) where the provider supports it:

| Provider | Live thinking | Effort setting |
|---|---|---|
| AICredits | Yes, for models that reason | Yes |
| Google Gemini | Yes, as thought summaries | Yes |
| OpenAI | No: OpenAI doesn't return reasoning on this API | Yes |
| Anthropic | No: Claude's thinking isn't returned on this API | No: Claude decides on its own |
| Custom | If the endpoint sends it | No |

Web search during tasks is optional and uses [Tavily](https://tavily.com).

---

## Getting started

### 1. Install

Build both apps from source (see [Building from source](#building-from-source)), or use the binaries from the Releases page if one is published.

### 2. Set up the PC app

1. Open **Vivy**.
2. Click **Settings**, choose a **provider**, and paste your API key.
3. Optionally add a Tavily key for web search.

### 3. Set up the phone app

1. Install **Vivy Agent** on your phone.
2. Turn on **Accessibility** for Vivy Agent when the app asks.
3. Allow **screen sharing**.
4. Enter the address and the **pairing code** shown in the PC app (for example `192.168.1.20` and `7KQ4-M2XD-9HPA`) and tap **Pair**. You only pair once; after that, just tap **Connect**.

Both devices must be on the same network. Over USB, you can instead run `adb reverse tcp:8765 tcp:8765` and connect to `127.0.0.1`.

### 4. Ask for something

```
Open YouTube and search for lo-fi music
Turn on Wi-Fi from quick settings
What's on my screen right now?
```

> **Tip:** Some phones (Xiaomi, Oppo, Vivo, Samsung and others) stop background services to save battery. If Android says the accessibility service is *not working*, set Vivy Agent's battery usage to **Unrestricted** and allow **Autostart**.

---

## Vivy in the terminal

The `vivy` command runs the same agent from a terminal on **Windows or Linux**. On a computer that also has the desktop app, both share the same settings, keys, chats and paired phone. Run one at a time, because both use port 8765 for the phone.

```bash
vivy key                 # save an API key (typed hidden)
vivy pair                # show the pairing code, wait for the phone
vivy                     # chat with Vivy
vivy run "turn on wi-fi" # do one task and exit (0 done, 1 failed, 130 stopped)
```

In a chat, type a request, or a command:

| Command | |
|---|---|
| `/model`, `/use`, `/effort`, `/key` | switch model or provider, set thinking effort, save a key |
| `/new`, `/chats`, `/open N`, `/retry` | chats, and resending your last message |
| `/pair`, `/phone`, `/forget` | phone pairing and status |
| `/matsumoto`, `/status`, `/help`, `/quit` | |

**Ctrl+C** stops a running task. Sensitive steps ask *Allow? [y/N]* right in the terminal.

The same commands work from the shell: `vivy status`, `vivy model 2`, `vivy use google`, `vivy chats`, `vivy chat --resume`, `vivy forget`. Run `vivy --help` for everything.

### Pixel Matsumoto

Matsumoto appears as pixel art drawn with colour block characters. He floats and blinks when Vivy starts, and turns back and forth while he works. You can change how he looks:

```bash
vivy matsumoto                         # say hi
vivy matsumoto --themes                # see every theme
vivy matsumoto --theme sakura --size large
vivy matsumoto --eye "#ff4f8b" --body "#fff6e9" --lens "#2a2a2e"
vivy matsumoto --style ascii           # for terminals without colour or Unicode
vivy matsumoto --animate off
vivy matsumoto --reset
```

Themes are `classic`, `night`, `sakura`, `mint`, `sunset` and `retro`, and the sizes are `tiny`, `small`, `medium` and `large`. Your choices are saved in `cli.toml` (see `vivy status` for where), which you can also edit by hand. That file also has `color = auto | truecolor | 256 | none` and `banner = true | false`. `NO_COLOR=1` always turns colour off.

### Settings from the environment

For scripts, CI or a server without a keyring, environment variables override the saved settings without changing them:

| Variable | |
|---|---|
| `VIVY_PROVIDER` | `aicredits`, `openai`, `anthropic`, `google` or `custom` |
| `VIVY_API_KEY` | key for that provider |
| `VIVY_BASE_URL` | endpoint for a custom provider (implies `custom`) |
| `VIVY_MODEL`, `VIVY_EFFORT` | model ID and thinking effort |
| `TAVILY_API_KEY` | web search |
| `VIVY_HOME` | keep all of Vivy's files in this folder instead |
| `VIVY_DEBUG=1` | also print diagnostics to stderr (they always go to `vivy.log`) |

### Where things are stored

| | Windows | Linux |
|---|---|---|
| Keys and settings | Windows Credential Manager | Secret Service (GNOME Keyring, KWallet); without one, a `config.json` only you can read |
| Chats, pairing, logs | `%LOCALAPPDATA%\Vivy` | `~/.local/share/vivy` |
| Terminal look (`cli.toml`) | `%APPDATA%\Vivy` | `~/.config/vivy` |

---

## How it works

```mermaid
graph LR
    subgraph PC["Your computer: desktop app or terminal (Rust)"]
        UI["Chat and live mirror<br/>(desktop) or terminal"]
        AGENT["Agent loop<br/>(Vivy plans, Matsumoto acts)"]
        WS["WebSocket server :8765"]
        VAULT["System keyring"]
    end
    subgraph CLOUD["Model provider"]
        LLM["Chat completions<br/>with tools + streaming"]
    end
    subgraph PHONE["Vivy Agent (Kotlin)"]
        ACC["Accessibility service<br/>(tap, swipe, type, read UI)"]
        CAP["Screen capture<br/>(in memory only)"]
    end
    UI <--> AGENT
    AGENT <--> LLM
    AGENT <--> WS
    VAULT --> AGENT
    WS <-->|"commands and results"| ACC
    CAP -->|"JPEG frames"| WS
```

1. Your message and the chat's memory go to the model, together with the list of tools Matsumoto can use.
2. The model answers in text or calls tools such as `inspect_screen`, `tap_element`, `open_app` or `type_text`.
3. The PC sends each command to the phone over the WebSocket, waits for the result, and feeds it back to the model.
4. The loop continues until the task is finished, you stop it, or a safety limit is reached.

The agent, the phone connection, settings and chats live in one shared Rust crate (`core/`). The desktop app and the terminal app are two front ends on top of it.

The message format between PC and phone is documented in [docs/PROTOCOL.md](docs/PROTOCOL.md).

---

## Privacy and safety

| | |
|---|---|
| **Screen frames** | Streamed in memory and never written to disk on either device. |
| **API keys** | Stored in the system keyring (see [where things are stored](#where-things-are-stored)). They are sent only to the provider you choose. |
| **Chats** | Saved locally in the `sessions` folder so you can reopen them. Delete a chat from the sidebar or with `/delete`. |
| **Sensitive actions** | Reading photos, files, messages or email requires your OK. Deleting things and payments ask every time. |
| **Stopping** | `Esc` in the window, `Ctrl+Shift+X` from anywhere, or the agent switch in the top bar. `Ctrl+C` in the terminal. |

**Secure pairing.** The PC and phone pair once with a one-time code, and from then on only that phone can connect to that PC:

- Everything between them (screen frames and commands) is encrypted with TLS.
- During pairing, both sides prove they know the code, tied to the PC's certificate, so another device on the network can't pose as either one.
- After pairing, the phone pins the PC's certificate and proves itself with a secret token. Any other connection is dropped before it receives anything.
- Codes are 12 characters, expire after 5 minutes, and stop working after 5 wrong tries.
- **Forget phone** on the PC (or **Forget this PC** on the phone) undoes the pairing.

---

## Building from source

### Requirements

- **Desktop app:** Windows 10 or 11, [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 18+.
- **Terminal app:** Rust (stable) and a C compiler, on Windows (Visual Studio Build Tools) or Linux (`build-essential` or your distro's equivalent). No other system libraries are needed.
- **Android:** Android Studio, or a JDK 17+ with the Android SDK. Phones need Android 7.0 or later for full gesture control.

### PC app

```bash
cd desktop
npm install
npm run tauri dev      # run with hot reload
npm run tauri build    # build Vivy.exe and the installers
```

The output goes to `target/release/` (the exe) and `target/release/bundle/` (MSI and setup installers).

### Terminal app

```bash
cargo build --release -p vivy-cli    # gives target/release/vivy (vivy.exe on Windows)
```

On Linux, copy it somewhere on your `PATH`, e.g. `install -m 755 target/release/vivy ~/.local/bin/`. Each push also builds it for Linux and Windows on GitHub Actions (see the workflow's artifacts).

### Android app

```bash
cd android
./gradlew assembleRelease
```

Release builds are signed with your own key if `android/keystore.properties` exists (copy [`keystore.properties.example`](android/keystore.properties.example)). Otherwise they use the Android debug key, which works fine for personal use.

### Both at once

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build_and_package.ps1
```

This builds the desktop app, the terminal app and the phone app, and copies them to `releases/`, which is gitignored.

### Tests

```bash
cargo test    # the engine and the terminal app
```

---

## Project layout

```
core/                    the engine, shared by both front ends
  src/agent/             agent loop, tools, vision, memory, permissions
  src/phone/             phone server, pairing and protocol
  src/security/          credential storage and provider settings
  src/sessions.rs        saved chats
cli/                     the `vivy` terminal app and pixel Matsumoto
desktop/                 the desktop app
  src/                   React UI (chat, mirror, settings, Matsumoto)
  src-tauri/             Tauri shell over the engine
android/                 phone app (Kotlin, Jetpack Compose)
  app/src/main/java/com/vivy/agent/
    service/             accessibility control and screen capture
    network/             WebSocket client
docs/                    protocol and setup guides
scripts/                 build and packaging script
```

---

## About the name

**Vivy** is the name of this project's assistant, and **Matsumoto** is the name of her helper, the cube that carries out each action. The names are a nod to *Vivy: Fluorite Eye's Song*, a story about an AI working alongside a helper AI to make people's lives better, which is what this project tries to do on a smaller scale.

Only the names are referenced. The project contains no artwork, music, dialogue, character designs or other material from the anime: the cube, the icon and all other assets were made for this project. Vivy is an independent open-source project and is not affiliated with or endorsed by the anime's creators or rights holders.

---

## License

[MIT](LICENSE). Use it, change it, share it.
