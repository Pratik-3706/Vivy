//! One-shot commands: settings, pairing, chats and Matsumoto's look.

use std::time::Duration;

use clap::Args;

use vivy_core::providers::{self, Provider};
use vivy_core::security::{credential_store_label, load_credentials_securely, load_saved_credentials, save_credentials_securely, ApiCredentials};
use vivy_core::sessions::SessionStore;
use vivy_core::state::{get_local_lan_ip, AppState};
use vivy_core::PHONE_PORT;

use crate::app::App;
use crate::config::{self, CliConfig, Look};
use crate::matsumoto;
use crate::term::{self, Rgb};

fn save(creds: &ApiCredentials) -> bool {
    match save_credentials_securely(creds) {
        Ok(()) => true,
        Err(e) => {
            println!("{}", term::red(&format!("✗ Couldn't save: {e}")));
            false
        }
    }
}

/// A change to saved settings that an environment variable will override.
fn warn_if_overridden(vars: &[&str]) {
    for v in vars {
        if std::env::var_os(v).is_some_and(|x| !x.is_empty()) {
            println!("{}", term::yellow(&format!("Note: {v} is set in this shell, so it overrides the saved setting.")));
        }
    }
}

fn current_provider(creds: &ApiCredentials) -> &'static Provider {
    providers::provider(&creds.provider)
}

fn model_label(p: &Provider, id: &str) -> String {
    match p.models.iter().find(|m| m.id == id) {
        Some(m) => format!("{} {}", m.name, term::dim(&format!("({})", m.id))),
        None => id.to_string(),
    }
}

// ---- status ---------------------------------------------------------------------

pub fn status() -> i32 {
    let creds = load_credentials_securely();
    let p = current_provider(&creds);
    let row = |k: &str, v: String| println!("  {:<10} {v}", term::dim(k));
    println!();
    row("provider", p.name.to_string());
    if p.id == "custom" {
        row("endpoint", creds.base_url.clone());
    }
    row("model", model_label(p, &creds.model_name));
    if !p.efforts.is_empty() {
        let effort = if creds.reasoning_effort.is_empty() { "default".to_string() } else { creds.reasoning_effort.clone() };
        row("effort", effort);
    }
    row("api key", if creds.api_key().is_empty() { term::yellow("not set · vivy key") } else { term::green("saved") });
    row("search", if creds.tavily_key.trim().is_empty() { term::dim("off · vivy key tavily") } else { term::green("on (Tavily)") });
    row("keys in", credential_store_label());

    let pairing = vivy_core::phone::pairing::Pairing::load().ok().map(|p| p.view());
    let phone = match pairing.as_ref().and_then(|v| v.paired.as_ref()) {
        Some(ph) => ph.device_name.clone(),
        None => term::dim("not paired · vivy pair"),
    };
    row("phone", phone);
    row("address", format!("{}:{PHONE_PORT}", get_local_lan_ip()));
    row("chats", format!("{}", SessionStore::load().list().len()));
    if let Some(d) = vivy_core::paths::data_dir() {
        row("data", d.display().to_string());
    }
    if let Some(c) = config::path() {
        row("look", c.display().to_string());
    }
    println!();
    0
}

// ---- keys and models ------------------------------------------------------------

pub fn key(which: Option<&str>) -> i32 {
    let mut creds = load_saved_credentials();
    let tavily = which.is_some_and(|w| w.eq_ignore_ascii_case("tavily"));
    let provider = if tavily {
        None
    } else {
        match which {
            Some(q) => match providers::find_provider(q) {
                Some(p) => Some(p),
                None => {
                    println!("{}", term::red(&format!("Unknown provider '{q}'. Try aicredits, openai, anthropic, google, custom or tavily.")));
                    return 2;
                }
            },
            None => Some(current_provider(&creds)),
        }
    };
    let (label, url) = match provider {
        Some(p) => (format!("{} API key", p.name), p.key_url),
        None => ("Tavily key (web search)".to_string(), "https://tavily.com"),
    };
    if !url.is_empty() {
        println!("{}", term::dim(&format!("Get one at {url}")));
    }
    let typed = match rpassword::prompt_password(format!("{label} (hidden, Enter to keep the current one): ")) {
        Ok(k) => k.trim().to_string(),
        Err(e) => {
            println!("{}", term::red(&format!("Couldn't read the key: {e}")));
            return 1;
        }
    };
    if typed.is_empty() {
        println!("{}", term::dim("Unchanged."));
        return 0;
    }
    match provider.map(|p| p.id) {
        None => creds.tavily_key = typed,
        Some("openai") => creds.openai_key = typed,
        Some("anthropic") => creds.anthropic_key = typed,
        Some("google") => creds.google_key = typed,
        Some("custom") => creds.custom_key = typed,
        Some(_) => creds.aicredits_key = typed,
    }
    if save(&creds) {
        println!("{} Saved in {}.", term::green("✓"), credential_store_label());
        warn_if_overridden(if tavily { &["TAVILY_API_KEY"] } else { &["VIVY_API_KEY"] });
        if let Some(p) = provider.filter(|p| p.id != creds.provider) {
            println!("{}", term::dim(&format!("You're using {} right now. Switch with: vivy use {}", current_provider(&creds).name, p.id)));
        }
        0
    } else {
        1
    }
}

pub fn use_provider(query: &str, model: Option<&str>, url: Option<&str>) -> i32 {
    let Some(p) = providers::find_provider(query) else {
        println!("{}", term::red(&format!("Unknown provider '{query}'. Try aicredits, openai, anthropic, google or custom.")));
        return 2;
    };
    let mut creds = load_saved_credentials();
    if let Some(u) = url {
        creds.base_url = u.trim().to_string();
    }
    if p.id == "custom" && creds.base_url.trim().is_empty() {
        println!("{}", term::red("The custom provider needs an endpoint: vivy use custom --url https://…/v1"));
        return 2;
    }
    creds.provider = p.id.to_string();
    let switching = !p.models.iter().any(|m| m.id == creds.model_name);
    if let Some(m) = model {
        creds.model_name = resolve_model(p, m);
    } else if switching && !p.models.is_empty() {
        creds.model_name = p.models[0].id.to_string();
    }
    if !p.efforts.contains(&creds.reasoning_effort.as_str()) {
        creds.reasoning_effort.clear();
    }
    if !save(&creds) {
        return 1;
    }
    println!("{} Using {} · {}", term::green("✓"), p.name, model_label(p, &creds.model_name));
    warn_if_overridden(&["VIVY_PROVIDER", "VIVY_BASE_URL", "VIVY_MODEL"]);
    if creds.api_key().is_empty() {
        println!("{}", term::yellow(&format!("No {} key saved yet. Add one with: vivy key {}", p.name, p.id)));
    }
    0
}

/// A list number ("2") or a model id.
fn resolve_model(p: &Provider, query: &str) -> String {
    query
        .parse::<usize>()
        .ok()
        .and_then(|n| p.models.get(n.checked_sub(1)?))
        .map(|m| m.id.to_string())
        .unwrap_or_else(|| query.trim().to_string())
}

pub fn model(choice: Option<&str>) -> i32 {
    let mut creds = if choice.is_some() { load_saved_credentials() } else { load_credentials_securely() };
    let p = current_provider(&creds);
    match choice {
        None => {
            println!("\n  {} models {}", term::bold(p.name), term::dim("· pick with: vivy model N  (or any model ID)"));
            for (i, m) in p.models.iter().enumerate() {
                let mark = if m.id == creds.model_name { term::green("●") } else { " ".into() };
                let note = format!("{:<14}", m.note);
                let tag = if m.complex {
                    term::yellow(&note)
                } else if m.everyday {
                    term::cyan(&note)
                } else {
                    term::dim(&note)
                };
                println!("  {mark} {:>2}  {:<24} {tag} {}", i + 1, m.name, term::dim(m.id));
            }
            if !p.models.iter().any(|m| m.id == creds.model_name) {
                println!("  {}     {}", term::green("●"), creds.model_name);
            }
            println!();
            0
        }
        Some(q) => {
            creds.model_name = resolve_model(p, q);
            if save(&creds) {
                println!("{} Model: {}", term::green("✓"), model_label(p, &creds.model_name));
                warn_if_overridden(&["VIVY_MODEL"]);
                0
            } else {
                1
            }
        }
    }
}

pub fn effort(level: Option<&str>) -> i32 {
    let mut creds = if level.is_some() { load_saved_credentials() } else { load_credentials_securely() };
    let p = current_provider(&creds);
    if p.efforts.is_empty() {
        println!("{}", term::dim(&format!("{} has no thinking-effort setting; the model decides.", p.name)));
        return 0;
    }
    match level {
        None => {
            let now = if creds.reasoning_effort.is_empty() { "default" } else { &creds.reasoning_effort };
            println!("  effort: {}   {}", term::bold(now), term::dim(&format!("options: default, {}", p.efforts.join(", "))));
            0
        }
        Some(l) => {
            let l = l.trim().to_ascii_lowercase();
            if l == "default" {
                creds.reasoning_effort.clear();
            } else if p.efforts.contains(&l.as_str()) {
                creds.reasoning_effort = l;
            } else {
                println!("{}", term::red(&format!("{} accepts: default, {}", p.name, p.efforts.join(", "))));
                return 2;
            }
            if save(&creds) {
                println!("{} Effort: {}", term::green("✓"), if creds.reasoning_effort.is_empty() { "default" } else { &creds.reasoning_effort });
                warn_if_overridden(&["VIVY_EFFORT"]);
                0
            } else {
                1
            }
        }
    }
}

// ---- chats ----------------------------------------------------------------------

pub fn chats() -> i32 {
    let list = SessionStore::load().list();
    if list.is_empty() {
        println!("{}", term::dim("No saved chats yet."));
        return 0;
    }
    println!();
    for (i, s) in list.iter().enumerate().take(30) {
        let when = chrono::DateTime::from_timestamp_millis(s.updated_at as i64)
            .map(|d| d.with_timezone(&chrono::Local).format("%b %d %H:%M").to_string())
            .unwrap_or_default();
        println!("  {:>2}  {:<50} {}", i + 1, s.title, term::dim(&when));
    }
    if list.len() > 30 {
        println!("  {}", term::dim(&format!("… and {} older", list.len() - 30)));
    }
    println!("\n  {}", term::dim("In a chat: /open N. From the shell: vivy chat --resume (most recent)."));
    0
}

// ---- pairing --------------------------------------------------------------------

/// Print the pairing code and address (used by `vivy pair` and `/pair`).
pub fn show_pairing(state: &AppState, fresh: bool) {
    let Some(pairing) = &state.phone.pairing else {
        println!("{}", term::red("Secure pairing isn't available on this machine (see vivy.log)."));
        return;
    };
    let view = if fresh { pairing.start() } else { pairing.view() };
    println!();
    if let Some(p) = &view.paired {
        println!("  {} Paired with {}", term::green("●"), term::bold(&p.device_name));
        if view.code.is_none() {
            println!("  {}", term::dim("To pair a different phone: vivy pair --new (or /pair new)"));
            println!();
            return;
        }
    }
    let code = view.code.unwrap_or_default();
    println!("  On the phone, open Vivy Agent and enter:");
    println!();
    println!("    {}  {}", term::dim("address"), term::bold(&format!("{}:{PHONE_PORT}", get_local_lan_ip())));
    println!("    {}  {}", term::dim("code   "), term::bold(&code));
    println!();
    println!("  {}", term::dim("The code works for 5 minutes. Phone and computer must be on the same network."));
    println!();
}

pub async fn pair(cfg: CliConfig, fresh: bool) -> i32 {
    let mut app = App::start(cfg);
    show_pairing(&app.state, fresh);
    let waiting = app.state.phone.pairing.as_ref().is_some_and(|p| p.view().code.is_some());
    if !waiting {
        return 0;
    }
    println!("  {}", term::dim("Waiting for the phone… (Ctrl+C to cancel)"));
    let deadline = tokio::time::sleep(vivy_core::phone::pairing::CODE_LIFETIME);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            ev = app.events.recv() => {
                let Some((name, payload)) = ev else { return 1 };
                if name == "pairing-changed" {
                    if let Some(n) = payload["paired"]["device_name"].as_str() {
                        println!("  {} Paired with {}. You're set: run {}", term::green("✓"), term::bold(n), term::bold("vivy"));
                        return 0;
                    }
                } else if name == "phone-error" {
                    if let Some(e) = payload["error"].as_str() {
                        println!("  {}", term::yellow(e));
                    }
                } else if name == "phone-firewall" {
                    if let Some(m) = payload["message"].as_str() {
                        println!("  {} Fix it with {} (in another terminal), then pair again.", term::yellow(m), term::bold("vivy firewall"));
                    }
                }
            }
            _ = &mut deadline => {
                println!("  {}", term::yellow("The code expired. Run vivy pair again for a new one."));
                return 1;
            }
            _ = tokio::signal::ctrl_c() => {
                println!("  {}", term::dim("Cancelled."));
                return 130;
            }
        }
    }
}

/// Check Windows Firewall for the phone port, and fix it through the admin prompt.
pub fn firewall() -> i32 {
    use vivy_core::firewall::{self, FirewallStatus};
    let status = firewall::check(vivy_core::PHONE_PORT);
    match status {
        FirewallStatus::Allowed => {
            println!("{} Windows Firewall already lets your phone reach Vivy.", term::green("✓"));
            return 0;
        }
        FirewallStatus::Unknown if !cfg!(windows) => {
            println!("Nothing to do here: firewall setup is only automatic on Windows.");
            return 0;
        }
        _ => {}
    }
    if status.needs_fix() {
        println!("{}", term::yellow(status.message()));
    }
    println!("Asking Windows for permission (an admin prompt will appear)…");
    match firewall::allow(vivy_core::PHONE_PORT) {
        Ok(FirewallStatus::Allowed) => {
            println!("{} Done. Your phone can connect now.", term::green("✓"));
            0
        }
        Ok(_) => {
            println!("{}", term::yellow("The rule was added, but Windows still doesn't report it as allowed. Try: vivy firewall"));
            1
        }
        Err(e) => {
            println!("{}", term::red(&e));
            1
        }
    }
}

pub fn forget() -> i32 {
    match vivy_core::phone::pairing::Pairing::load() {
        Ok(p) => {
            p.forget();
            println!("{} Phone unpaired. Pair again with: vivy pair", term::green("✓"));
            0
        }
        Err(e) => {
            println!("{}", term::red(&e));
            1
        }
    }
}

pub async fn phone_status(state: &AppState) {
    let d = state.phone.get_device_state().await;
    if !d.connected {
        let paired = state.phone.pairing.as_ref().and_then(|p| p.view().paired);
        match paired {
            Some(p) => println!("  {} {} is paired but not connected. Open Vivy Agent on the phone.", term::yellow("○"), p.device_name),
            None => println!("  {} No phone paired. Type /pair.", term::yellow("○")),
        }
        return;
    }
    let flag = |v: Option<bool>| match v {
        Some(true) => term::green("on"),
        Some(false) => term::red("off"),
        None => term::dim("unknown"),
    };
    println!("  {} {} {} · Android {} · battery {}%{}", term::green("●"), d.brand, d.model, d.android_version, d.battery_level, if d.is_charging { " (charging)" } else { "" });
    println!("    accessibility {}   screen sharing {}", flag(d.accessibility), flag(d.screen_share));
}

// ---- Matsumoto ------------------------------------------------------------------

#[derive(Args, Default)]
pub struct MatsumotoArgs {
    /// Colour theme: classic, night, sakura, mint, sunset, retro.
    #[arg(long)]
    pub theme: Option<String>,
    /// Shell colour, #rrggbb ("theme" to use the theme's).
    #[arg(long)]
    pub body: Option<String>,
    /// Eye colour, #rrggbb ("theme" to use the theme's).
    #[arg(long)]
    pub eye: Option<String>,
    /// Lens (the ring around the eye), #rrggbb ("theme" to use the theme's).
    #[arg(long)]
    pub lens: Option<String>,
    /// tiny, small, medium or large.
    #[arg(long)]
    pub size: Option<String>,
    /// pixel (colour blocks) or ascii.
    #[arg(long)]
    pub style: Option<String>,
    /// Turn animation on or off.
    #[arg(long, value_name = "on|off")]
    pub animate: Option<String>,
    /// Show every theme side by side.
    #[arg(long)]
    pub themes: bool,
    /// Back to the defaults.
    #[arg(long)]
    pub reset: bool,
    /// Print every pose side by side (for checking the drawing).
    #[arg(long, hide = true)]
    pub sheet: bool,
}

pub async fn matsumoto(mut cfg: CliConfig, a: MatsumotoArgs) -> i32 {
    if a.themes {
        show_themes(&cfg);
        return 0;
    }
    if a.sheet {
        let look = cfg.matsumoto.look();
        let poses: Vec<(String, matsumoto::Pose)> = [
            ("ahead", matsumoto::Pose::default()),
            ("look left", matsumoto::Pose { look: -1, ..Default::default() }),
            ("look right", matsumoto::Pose { look: 1, ..Default::default() }),
            ("blink", matsumoto::Pose { blink: true, ..Default::default() }),
            ("float", matsumoto::Pose { lift: 1, ..Default::default() }),
        ]
        .into_iter()
        .map(|(n, p)| (n.to_string(), p))
        .chain([-1.0f32, -0.6, -0.3, 0.3, 0.6, 1.0].into_iter().map(|t| (format!("turn {t}"), matsumoto::Pose { turn: t, ..Default::default() })))
        .collect();
        let columns: Vec<Vec<String>> = poses
            .iter()
            .map(|(name, pose)| {
                let mut l = matsumoto::to_lines(&matsumoto::draw(&look.palette, look.edge, *pose), term::mode(), look.pixel);
                l.push(term::dim(name));
                l
            })
            .collect();
        print_columns(&columns, matsumoto::canvas_size(look.edge).0 + 3);
        return 0;
    }
    let m = &mut cfg.matsumoto;
    let mut changed = false;
    if a.reset {
        *m = Default::default();
        changed = true;
    }
    if let Some(t) = a.theme {
        if matsumoto::theme(&t).is_none() {
            let names: Vec<_> = matsumoto::THEMES.iter().map(|t| t.name).collect();
            println!("{}", term::red(&format!("Unknown theme '{t}'. Themes: {}", names.join(", "))));
            return 2;
        }
        m.theme = t.to_ascii_lowercase();
        // A new theme brings its own colours unless set in the same command.
        m.body = None;
        m.eye = None;
        m.lens = None;
        changed = true;
    }
    for (value, slot, part) in [(a.body, &mut m.body, "body"), (a.eye, &mut m.eye, "eye"), (a.lens, &mut m.lens, "lens")] {
        let Some(v) = value else { continue };
        if v.eq_ignore_ascii_case("theme") {
            *slot = None;
        } else if let Some(c) = Rgb::parse(&v) {
            *slot = Some(c.hex());
        } else {
            println!("{}", term::red(&format!("--{part} needs a colour like #3d8bff (got '{v}').")));
            return 2;
        }
        changed = true;
    }
    if let Some(s) = a.size {
        if matsumoto::edge_for(&s).is_none() {
            println!("{}", term::red(&format!("Sizes: {}", matsumoto::SIZES.join(", "))));
            return 2;
        }
        m.size = s.to_ascii_lowercase();
        changed = true;
    }
    if let Some(s) = a.style {
        let s = s.to_ascii_lowercase();
        if s != "pixel" && s != "ascii" {
            println!("{}", term::red("Styles: pixel, ascii"));
            return 2;
        }
        m.style = s;
        changed = true;
    }
    if let Some(v) = a.animate {
        m.animate = matches!(v.to_ascii_lowercase().as_str(), "on" | "true" | "yes" | "1");
        changed = true;
    }

    let look = cfg.matsumoto.look();
    preview(look, if changed { Duration::from_secs(3) } else { Duration::from_secs(6) }).await;
    if changed {
        match cfg.save() {
            Ok(path) => println!("{} Saved to {}", term::green("✓"), path.display()),
            Err(e) => {
                println!("{}", term::red(&e));
                return 1;
            }
        }
    } else {
        let m = &cfg.matsumoto;
        let theme = matsumoto::theme(&m.theme).map(|t| t.name).unwrap_or("classic");
        println!(
            "  {}",
            term::dim(&format!(
                "theme {theme} · size {} · {} · animation {}",
                m.size,
                m.style,
                if m.animate { "on" } else { "off" }
            ))
        );
        println!("  {}", term::dim("Change him: vivy matsumoto --theme sakura --size large --eye \"#ff4f8b\"  ·  --themes to see all"));
    }
    0
}

/// Matsumoto idling, then busy, in place, for `length`.
pub async fn preview(look: Look, length: Duration) {
    let mode = term::mode();
    let animate = look.animate && term::is_tty() && mode != term::ColorMode::None;
    let frames = if animate { (length.as_millis() / 125) as u64 } else { 1 };
    let mut drawn = 0usize;
    println!();
    for t in 0..frames.max(1) {
        let busy = animate && t >= frames / 2;
        let pose = if !animate {
            matsumoto::Pose::default()
        } else if busy {
            matsumoto::busy_pose(t)
        } else {
            matsumoto::idle_pose(t)
        };
        let mut lines = matsumoto::to_lines(&matsumoto::draw(&look.palette, look.edge, pose), mode, look.pixel);
        let caption = if !animate {
            "Matsumoto"
        } else if busy {
            "Matsumoto, working"
        } else {
            "Matsumoto, idle"
        };
        lines.push(String::new());
        lines.push(term::dim(caption));
        if drawn > 0 {
            print!("\x1b[{drawn}A");
        }
        for l in &lines {
            println!("\x1b[2K  {l}");
        }
        drawn = lines.len();
        if animate {
            tokio::time::sleep(Duration::from_millis(125)).await;
        }
    }
    println!();
}

fn show_themes(cfg: &CliConfig) {
    let mode = term::mode();
    let pixel = cfg.matsumoto.look().pixel;
    let edge = 12;
    let columns: Vec<Vec<String>> = matsumoto::THEMES
        .iter()
        .map(|t| {
            let palette = matsumoto::Palette::new(t.body, t.eye, t.lens);
            let mut lines = matsumoto::to_lines(&matsumoto::draw(&palette, edge, matsumoto::Pose::default()), mode, pixel);
            lines.push(term::bold(t.name));
            lines
        })
        .collect();
    print_columns(&columns, matsumoto::canvas_size(edge).0 + 4);
    for t in matsumoto::THEMES {
        println!("  {:<8} {}", term::bold(t.name), term::dim(t.blurb));
    }
    println!("\n  {}", term::dim("Pick one: vivy matsumoto --theme NAME"));
}

/// Blocks of lines laid out in columns, wrapping to the terminal width.
fn print_columns(columns: &[Vec<String>], col_w: usize) {
    let per_row = (term::width() / col_w).clamp(1, columns.len().max(1));
    println!();
    for chunk in columns.chunks(per_row) {
        let rows = chunk.iter().map(|c| c.len()).max().unwrap_or(0);
        for r in 0..rows {
            let mut line = String::from("  ");
            for col in chunk {
                let cell = col.get(r).cloned().unwrap_or_default();
                let pad = col_w.saturating_sub(term::visible_width(&cell));
                line.push_str(&cell);
                line.push_str(&" ".repeat(pad));
            }
            println!("{}", line.trim_end());
        }
        println!();
    }
}
