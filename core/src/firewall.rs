//! Windows Firewall check for the phone connection.
//!
//! Windows drops incoming connections for programs it has no rule for, and the
//! phone just sees "didn't answer in time". The desktop app usually gets a rule
//! from the "Allow access" prompt, but the CLI often doesn't, and a rule made
//! for one copy of the program doesn't cover a copy at another path. So check
//! for this program, and offer a fix that goes through the normal admin prompt.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallStatus {
    /// An enabled rule lets this program (or the port) through on the current network.
    Allowed,
    /// A rule explicitly blocks this program.
    Blocked,
    /// No rule covers this program on the current network, so Windows drops connections.
    Missing,
    /// Not Windows, or the check itself failed.
    Unknown,
}

impl FirewallStatus {
    pub fn needs_fix(self) -> bool {
        matches!(self, Self::Blocked | Self::Missing)
    }

    /// One sentence for the user.
    pub fn message(self) -> &'static str {
        match self {
            Self::Blocked => "Windows Firewall is blocking Vivy, so your phone can't connect.",
            Self::Missing => "Windows Firewall hasn't allowed Vivy on this network, so your phone can't connect.",
            Self::Allowed | Self::Unknown => "",
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::FirewallStatus;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// Keeps a console window from flashing up when the desktop app runs PowerShell.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn this_exe() -> Option<String> {
        let path = std::env::current_exe().ok()?;
        Some(path.to_string_lossy().replace('\'', "''"))
    }

    fn powershell(script: &str) -> std::io::Result<std::process::Output> {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    /// PowerShell takes the script as UTF-16LE base64, which avoids every quoting problem.
    fn encode(script: &str) -> String {
        let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    }

    pub fn check(port: u16) -> FirewallStatus {
        match this_exe() {
            Some(exe) => check_program(&exe, port),
            None => FirewallStatus::Unknown,
        }
    }

    /// `exe` must already have single quotes doubled for PowerShell.
    pub fn check_program(exe: &str, port: u16) -> FirewallStatus {
        // Reading rules needs no admin rights. Profiles: the rule applies if it
        // is for "Any" or names the category of a network we're connected to.
        let script = format!(
            r#"
$ErrorActionPreference = 'SilentlyContinue'
$exe = '{exe}'
$cats = @(Get-NetConnectionProfile | ForEach-Object {{ if ($_.NetworkCategory -eq 'DomainAuthenticated') {{ 'Domain' }} else {{ "$($_.NetworkCategory)" }} }})
function Applies($r) {{
  if ($r.Enabled -ne 'True' -or $r.Direction -ne 'Inbound') {{ return $false }}
  $p = "$($r.Profile)"
  if ($p -eq 'Any' -or $cats.Count -eq 0) {{ return $true }}
  foreach ($c in $cats) {{ if ($p -match $c) {{ return $true }} }}
  return $false
}}
$mine = @(Get-NetFirewallApplicationFilter -PolicyStore ActiveStore | Where-Object {{ $_.Program -ieq $exe }} | Get-NetFirewallRule | Where-Object {{ Applies $_ }})
if ($mine | Where-Object {{ $_.Action -eq 'Block' }}) {{ 'blocked'; exit }}
if ($mine | Where-Object {{ $_.Action -eq 'Allow' }}) {{ 'allowed'; exit }}
$port = @(Get-NetFirewallPortFilter -PolicyStore ActiveStore -Protocol TCP | Where-Object {{ $_.LocalPort -eq '{port}' }} | Get-NetFirewallRule | Where-Object {{ (Applies $_) -and $_.Action -eq 'Allow' }})
if ($port.Count -gt 0) {{ 'allowed' }} else {{ 'missing' }}
"#
        );
        match powershell(&script) {
            Ok(out) => match String::from_utf8_lossy(&out.stdout).trim() {
                "allowed" => FirewallStatus::Allowed,
                "blocked" => FirewallStatus::Blocked,
                "missing" => FirewallStatus::Missing,
                _ => FirewallStatus::Unknown,
            },
            Err(_) => FirewallStatus::Unknown,
        }
    }

    /// Adds an allow rule for this program on the phone port (and removes rules
    /// that block it). Shows the Windows admin prompt; returns an error if the
    /// user declines it.
    pub fn allow(port: u16) -> Result<(), String> {
        let exe = this_exe().ok_or("Couldn't find Vivy's own program file.")?;
        let fix = format!(
            r#"
$exe = '{exe}'
Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -ieq $exe }} | Get-NetFirewallRule |
  Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' }} | Remove-NetFirewallRule
Get-NetFirewallRule -DisplayName 'Vivy (phone connection)' -ErrorAction SilentlyContinue |
  Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -ieq $exe }} | Get-NetFirewallRule | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'Vivy (phone connection)' -Description 'Lets your paired phone connect to Vivy.' `
  -Direction Inbound -Action Allow -Program $exe -Protocol TCP -LocalPort {port} -Profile Any | Out-Null
"#
        );
        // Start-Process -Verb RunAs is what shows the admin (UAC) prompt.
        let launcher = format!(
            "try {{ Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}'; 'ok' }} catch {{ 'declined' }}",
            encode(&fix)
        );
        let out = powershell(&launcher).map_err(|e| format!("Couldn't run PowerShell: {e}"))?;
        if String::from_utf8_lossy(&out.stdout).trim() == "declined" {
            return Err("The admin prompt was cancelled, so the firewall wasn't changed.".into());
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::FirewallStatus;

    pub fn check(_port: u16) -> FirewallStatus {
        FirewallStatus::Unknown
    }

    pub fn allow(_port: u16) -> Result<(), String> {
        Err("Firewall setup is only automatic on Windows. Allow incoming TCP on the phone port in your firewall.".into())
    }
}

/// Whether Windows Firewall lets phones reach this program on `port`. Slow
/// (runs PowerShell, about a second); call it off the async runtime.
pub fn check(port: u16) -> FirewallStatus {
    imp::check(port)
}

/// Allow this program through Windows Firewall, via the admin prompt, then
/// check again. Blocking; call it off the async runtime.
pub fn allow(port: u16) -> Result<FirewallStatus, String> {
    imp::allow(port)?;
    Ok(imp::check(port))
}

