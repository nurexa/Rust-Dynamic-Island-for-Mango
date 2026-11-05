//! The battery, from UPower's display device (all batteries combined), by signal.
//! On a desktop the display device isn't present and this thread ends at once.
use crate::Msg;
use std::{collections::HashMap, thread, time::Duration};
use zbus::{
    blocking::{MessageIterator, connection::Builder},
    message::Type,
    zvariant::{OwnedValue, Value},
};

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const PROPS: &str = "org.freedesktop.DBus.Properties";

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub pct: f64,
    pub charging: bool,
    /// On mains: charging, full, or holding off charging.
    pub plugged: bool,
    /// Seconds until empty (on battery) or full (charging); 0 when UPower doesn't know yet.
    pub secs: i64,
}

impl Battery {
    /// "2h 10m left", "45m to full", or nothing while UPower is still estimating.
    pub fn time(&self) -> Option<String> {
        if self.secs <= 0 {
            return None;
        }
        let (h, m) = (self.secs / 3600, self.secs % 3600 / 60);
        let t = if h > 0 { format!("{h}h {m}m") } else { format!("{m}m") };
        Some(if self.charging { format!("{t} to full") } else { format!("{t} left") })
    }
}

/// From UPower's property map. None when there's no battery behind the display device.
fn from(props: &HashMap<String, OwnedValue>) -> Option<Battery> {
    let get = |k: &str| props.get(k).map(|v| &**v);
    let present = matches!(get("IsPresent"), Some(Value::Bool(true)));
    // Type 2 is a battery; the display device also stands in for UPS-only machines.
    if !present || !matches!(get("Type"), Some(Value::U32(2))) {
        return None;
    }
    let num = |k: &str| match get(k) {
        Some(Value::F64(v)) => *v,
        Some(Value::I64(v)) => *v as f64,
        _ => 0.0,
    };
    // 1 charging, 2 discharging, 3 empty, 4 full, 5 pending charge, 6 pending discharge.
    let state = match get("State") { Some(Value::U32(s)) => *s, _ => 0 };
    let charging = state == 1;
    let secs = if charging { num("TimeToFull") } else { num("TimeToEmpty") };
    Some(Battery { pct: num("Percentage"), charging, plugged: matches!(state, 1 | 4 | 5), secs: secs as i64 })
}

pub fn spawn(tx: crate::Tx) {
    thread::spawn(move || {
        let Ok(conn) = Builder::system().and_then(|b| b.method_timeout(Duration::from_secs(2)).build()) else { return };
        let read = || -> Option<Battery> {
            let reply = conn.call_method(Some(UPOWER), DISPLAY, Some(PROPS), "GetAll", &DEVICE).ok()?;
            from(&reply.body().deserialize::<HashMap<String, OwnedValue>>().ok()?)
        };
        let Some(first) = read() else { return };
        let rule = format!("type='signal',sender='{UPOWER}',path='{DISPLAY}',interface='{PROPS}'");
        let _ = conn.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "AddMatch", &rule);
        let signals = MessageIterator::from(&conn);
        let _ = tx.send(Msg::Battery(first.clone()));
        let mut last = first;
        // Replies to our own calls come through here too: only signals count.
        for msg in signals.flatten() {
            if msg.message_type() != Type::Signal {
                continue;
            }
            if let Some(now) = read().filter(|n| *n != last) {
                last = now.clone();
                let _ = tx.send(Msg::Battery(now));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, Value)]) -> HashMap<String, OwnedValue> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.try_clone().unwrap().try_into().unwrap())).collect()
    }

    #[test]
    fn reads_upower_and_ignores_a_desktop() {
        let b = from(&props(&[("IsPresent", Value::Bool(true)), ("Type", Value::U32(2)), ("State", Value::U32(2)),
                              ("Percentage", Value::F64(42.0)), ("TimeToEmpty", Value::I64(7800))])).unwrap();
        assert_eq!((b.pct, b.charging, b.plugged), (42.0, false, false));
        assert_eq!(b.time().as_deref(), Some("2h 10m left"));
        let full = from(&props(&[("IsPresent", Value::Bool(true)), ("Type", Value::U32(2)), ("State", Value::U32(4)),
                                 ("Percentage", Value::F64(100.0))])).unwrap();
        assert!(full.plugged && !full.charging && full.time().is_none());
        assert_eq!(from(&props(&[("IsPresent", Value::Bool(false)), ("Type", Value::U32(2))])), None);
    }
}
