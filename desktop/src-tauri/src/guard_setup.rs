//! Stolen protection setup, run from this computer over USB (D-55).
//!
//! Android gives its strongest protections (power menu blocked in stolen mode, USB data
//! off, factory reset and safe mode blocked, Wi-Fi and location kept on) only to the
//! phone's **device owner** app. An app becomes device owner once, through Android's own
//! debugging tool:
//!
//! ```text
//! adb shell dpm set-device-owner app.devti.android/.guard.GuardAdmin
//! ```
//!
//! Android only allows it while the phone has no accounts signed in, so the guide asks
//! the user to remove them for a minute and add them back afterwards. This module finds
//! `adb`, lists phones plugged in over USB, runs that one command, and turns Android's
//! errors into plain instructions.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

const COMPONENT: &str = "app.devti.android/.guard.GuardAdmin";

#[derive(Clone, Debug, Serialize)]
pub struct UsbPhone {
    pub serial: String,
    /// "device" (ready), "unauthorized" (allow USB debugging on the phone), "offline".
    pub state: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupCheck {
    /// Where adb was found (empty: not installed).
    pub adb: String,
    /// How to install adb on this system.
    pub install_hint: String,
    pub phones: Vec<UsbPhone>,
}

fn candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut out = vec![PathBuf::from(exe)];
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(sdk) = std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT")) {
        out.push(PathBuf::from(sdk).join("platform-tools").join(exe));
    }
    if let Some(h) = &home {
        out.push(h.join("Android/Sdk/platform-tools").join(exe));
        out.push(h.join("platform-tools").join(exe));
        out.push(h.join("Downloads/platform-tools").join(exe));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("Android/Sdk/platform-tools").join(exe));
    }
    out
}

fn command(path: &PathBuf) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    c
}

fn find_adb() -> Option<PathBuf> {
    candidates().into_iter().find(|p| command(p).arg("version").output().map(|o| o.status.success()).unwrap_or(false))
}

fn install_hint() -> String {
    if cfg!(windows) {
        "Download Android's “SDK Platform-Tools” for Windows from developer.android.com/tools/releases/platform-tools, unzip it into your Downloads folder, then press Check again.".into()
    } else {
        let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
        if os.contains("fedora") {
            "Install it with: sudo dnf install android-tools".into()
        } else if os.contains("arch") {
            "Install it with: sudo pacman -S android-tools".into()
        } else {
            "Install it with: sudo apt install adb".into()
        }
    }
}

/// Is adb here, and which phones are plugged in?
pub fn check() -> SetupCheck {
    let Some(adb) = find_adb() else {
        return SetupCheck { adb: String::new(), install_hint: install_hint(), phones: vec![] };
    };
    let out = command(&adb).args(["devices", "-l"]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let phones = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let model = l
                .split_whitespace()
                .find_map(|w| w.strip_prefix("model:"))
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| serial.clone());
            Some(UsbPhone { serial, state, model })
        })
        .collect();
    SetupCheck { adb: adb.to_string_lossy().into(), install_hint: install_hint(), phones }
}

/// Makes Devti the device owner of the phone `serial`. Ok: a short success message.
pub fn make_owner(serial: &str) -> Result<String, String> {
    let adb = find_adb().ok_or_else(|| format!("adb isn't installed. {}", install_hint()))?;
    // Is Devti installed on it, and already the owner?
    let owners = command(&adb)
        .args(["-s", serial, "shell", "dumpsys", "device_policy"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if owners.contains("app.devti.android/.guard.GuardAdmin") && owners.contains("Device Owner") {
        return Ok("Devti already protects this phone.".into());
    }
    let pkgs = command(&adb)
        .args(["-s", serial, "shell", "pm", "list", "packages", "app.devti.android"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if !pkgs.contains("app.devti.android") {
        return Err("Devti isn't installed on that phone. Install the Devti APK on it first.".into());
    }
    let out = command(&adb)
        .args(["-s", serial, "shell", "dpm", "set-device-owner", COMPONENT])
        .output()
        .map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if text.contains("Success") {
        return Ok("Done. Devti can now fully protect this phone. Add your accounts back on the phone.".into());
    }
    Err(explain(&text))
}

/// Android's `dpm` errors, in plain words.
pub fn explain(text: &str) -> String {
    let t = text.to_lowercase();
    if t.contains("account") {
        "The phone still has accounts signed in. On the phone: Settings → Accounts (or Passwords & accounts), remove every account for a minute, then press Make Devti the protector again. Add them back afterwards.".into()
    } else if t.contains("already several users") || t.contains("multiple users") || t.contains("users on the device") {
        "The phone has more than one user or a work profile (or Samsung Secure Folder / Xiaomi Second space). Remove the extra user or profile, then try again.".into()
    } else if t.contains("device owner is already set")
        || t.contains("already has a device owner")
        || t.contains("already provisioned")
    {
        "Another app already manages this phone (a work or school app). Only one app can be the protector.".into()
    } else if t.contains("unauthorized") {
        "Allow USB debugging on the phone: unlock it and tap Allow on the “Allow USB debugging?” question.".into()
    } else if t.contains("security exception") && t.contains("usb debugging") {
        "On Xiaomi phones also turn on “USB debugging (Security settings)” in Developer options, then try again.".into()
    } else if t.contains("not found") || t.contains("no devices") {
        "The phone isn't connected. Plug it in with a USB cable and turn on USB debugging.".into()
    } else {
        format!("Android said: {}", text.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::explain;

    #[test]
    fn android_errors_become_instructions() {
        assert!(explain("java.lang.IllegalStateException: Not allowed to set the device owner because there are already some accounts on the device").contains("remove every account"));
        assert!(
            explain("Not allowed to set the device owner because there are already several users on the device")
                .contains("more than one user")
        );
        assert!(explain("Trying to set the device owner, but device owner is already set.").contains("Another app"));
        assert!(explain("something new").starts_with("Android said:"));
    }
}
