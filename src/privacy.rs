//! Who has the camera open. inotify reports every open and close of /dev/video*,
//! so this sleeps in read(2) until one happens, and looks in /proc for holders only
//! when a device stays open.
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
        // Browsers and chat apps probe cameras every second or so: open, close. So
        // opens and closes are counted, and /proc is only searched when one stays open.
        let mut last = holders(&devices);
        let mut open = last.len() as i64;
        if !last.is_empty() {
            let _ = tx.send(Msg::Camera(last.clone()));
        }
        let mut buf = [0u8; 4096];
        loop {
            let n = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if n <= 0 {
                return;
            }
            // Gather a burst, so a constant prober costs one wakeup per BATCH.
            thread::sleep(BATCH);
            let mut events = count(&buf[..n as usize]);
            libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK);
            loop {
                let more = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
                if more <= 0 {
                    break;
                }
                let (o, c) = count(&buf[..more as usize]);
                events = (events.0 + o, events.1 + c);
            }
            libc::fcntl(fd, libc::F_SETFL, 0);
            open = (open + events.0 - events.1).max(0);
            let now = if open > 0 { holders(&devices) } else { Vec::new() };
            if now.is_empty() {
                open = 0; // whatever we thought, nobody holds one now
            }
            if now != last {
                last = now.clone();
                let _ = tx.send(Msg::Camera(now));
            }
        }
    });
}

const BATCH: Duration = Duration::from_secs(2);

/// Opens and closes in a buffer of inotify events.
fn count(buf: &[u8]) -> (i64, i64) {
    let (mut opens, mut closes, mut at) = (0, 0, 0);
    while at + 16 <= buf.len() {
        let mask = u32::from_ne_bytes(buf[at + 4..at + 8].try_into().unwrap_or_default());
        let len = u32::from_ne_bytes(buf[at + 12..at + 16].try_into().unwrap_or_default()) as usize;
        if mask & libc::IN_OPEN != 0 {
            opens += 1;
        }
        if mask & libc::IN_CLOSE != 0 {
            closes += 1;
        }
        at += 16 + len;
    }
    (opens, closes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_opens_and_closes_in_an_event_buffer() {
        let ev = |mask: u32| [1i32.to_ne_bytes(), mask.to_ne_bytes(), 0u32.to_ne_bytes(), 0u32.to_ne_bytes()].concat();
        let buf = [ev(libc::IN_OPEN), ev(libc::IN_CLOSE_NOWRITE), ev(libc::IN_OPEN)].concat();
        assert_eq!(count(&buf), (2, 1));
    }
}
