//! Who has the camera open. inotify reports every open and close of /dev/video*,
//! so this sleeps in read(2) until one happens, then looks in /proc for holders.
//! The microphone comes from audio.rs, which already hears every recording start.
use crate::Msg;
use std::{ffi::CString, fs, thread, time::Duration};

pub fn spawn(tx: crate::Tx) {
    let devices: Vec<String> = fs::read_dir("/dev").into_iter().flatten().flatten()
        .map(|e| e.path().to_string_lossy().into_owned())
        .filter(|p| p.starts_with("/dev/video")).collect();
    if devices.is_empty() {
        return;
    }
    thread::spawn(move || unsafe {
        let fd = libc::inotify_init1(libc::IN_CLOEXEC);
        if fd < 0 {
            return;
        }
        for d in &devices {
            let Ok(path) = CString::new(d.as_str()) else { continue };
            libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_OPEN | libc::IN_CLOSE);
        }
        let mut last = Vec::new();
        let mut buf = [0u8; 4096];
        while libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) > 0 {
            // Probing opens and closes in a burst; look once it settles.
            thread::sleep(Duration::from_millis(250));
            let now = holders(&devices);
            if now != last {
                last = now.clone();
                let _ = tx.send(Msg::Camera(now));
            }
        }
    });
}

/// Names of processes with a camera device open. Processes of other users can't
/// be looked into, which is fine: they're not on this desktop.
fn holders(devices: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for proc_dir in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let pid = proc_dir.file_name();
        if !pid.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let fds = proc_dir.path().join("fd");
        let open = fs::read_dir(&fds).into_iter().flatten().flatten()
            .any(|f| fs::read_link(f.path()).is_ok_and(|t| devices.iter().any(|d| t.as_os_str() == d.as_str())));
        if open {
            let comm = fs::read_to_string(proc_dir.path().join("comm")).unwrap_or_default().trim().to_owned();
            if !names.contains(&comm) {
                names.push(comm);
            }
        }
    }
    names.sort();
    names
}

/// The tip on the pill's dot: who has what.
pub fn describe(mic: &[String], cam: &[String]) -> String {
    let mut parts = Vec::new();
    if !mic.is_empty() {
        parts.push(format!("Microphone: {}", mic.join(", ")));
    }
    if !cam.is_empty() {
        let cam: Vec<&str> = cam.iter().map(|c| if matches!(c.as_str(), "pipewire" | "wireplumber") { "an app through PipeWire" } else { c }).collect();
        parts.push(format!("Camera: {}", cam.join(", ")));
    }
    parts.join(" · ")
}
