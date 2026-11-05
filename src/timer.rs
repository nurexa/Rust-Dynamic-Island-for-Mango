//! Pomodoro: focus, a short break, and a long one every few rounds. Or a plain
//! countdown from `island timer 10`. Driven by the main loop's deadline, so a
//! running timer wakes the island once a minute (once a second near the end, or
//! while the panel is open) and an idle one never does.
use crate::settings::Pomodoro;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Focus,
    Short,
    Long,
    /// A plain countdown: nothing follows it.
    Countdown,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::Short => "Short break",
            Phase::Long => "Long break",
            Phase::Countdown => "Timer",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    pub phase: Phase,
    /// 1-based focus round.
    pub round: u32,
    pub total: Duration,
    ends: Instant,
    /// Time left when paused.
    paused: Option<Duration>,
}

fn mins(m: u64) -> Duration {
    Duration::from_secs(m.max(1) * 60)
}

impl Timer {
    pub fn new(phase: Phase, round: u32, total: Duration, now: Instant) -> Self {
        Timer { phase, round, total, ends: now + total, paused: None }
    }

    pub fn focus(cfg: &Pomodoro, now: Instant) -> Self {
        Timer::new(Phase::Focus, 1, mins(cfg.focus), now)
    }

    pub fn left(&self, now: Instant) -> Duration {
        self.paused.unwrap_or_else(|| self.ends.saturating_duration_since(now))
    }

    pub fn paused(&self) -> bool {
        self.paused.is_some()
    }

    pub fn done(&self, now: Instant) -> bool {
        self.paused.is_none() && now >= self.ends
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        match self.paused.take() {
            Some(left) => self.ends = now + left,
            None => self.paused = Some(self.left(now)),
        }
    }

    /// What comes after this phase, if anything.
    pub fn next(&self, cfg: &Pomodoro, now: Instant) -> Option<Timer> {
        let (phase, round, m) = match self.phase {
            Phase::Focus if self.round >= cfg.rounds.max(1) => (Phase::Long, self.round, cfg.long),
            Phase::Focus => (Phase::Short, self.round, cfg.short),
            Phase::Short => (Phase::Focus, self.round + 1, cfg.focus),
            Phase::Long => (Phase::Focus, 1, cfg.focus),
            Phase::Countdown => return None,
        };
        Some(Timer::new(phase, round, mins(m), now))
    }

    /// When the pill's text next changes: each minute, then each second in the last
    /// one or while `fine` (the panel is showing seconds).
    pub fn next_change(&self, now: Instant, fine: bool) -> Option<Instant> {
        if self.paused.is_some() {
            return None;
        }
        let left = self.left(now);
        let step = if fine || left <= Duration::from_secs(60) { 1 } else { 60 };
        let into = left.as_millis() as u64 % (step * 1000);
        Some(now + Duration::from_millis(if into == 0 { step * 1000 } else { into }).min(left))
    }

    /// "24m" on the pill, then "0:42" for the last minute.
    pub fn short(&self, now: Instant) -> String {
        let s = self.left(now).as_secs_f64().ceil() as u64;
        if s >= 60 { format!("{}m", s.div_ceil(60)) } else { format!("0:{s:02}") }
    }

    /// "24:13" in the panel.
    pub fn clock(&self, now: Instant) -> String {
        let s = self.left(now).as_secs_f64().ceil() as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }

    pub fn progress(&self, now: Instant) -> f64 {
        1.0 - self.left(now).as_secs_f64() / self.total.as_secs_f64().max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_cycle_through_breaks_and_pausing_holds_the_time() {
        let cfg = Pomodoro { focus: 25, short: 5, long: 15, rounds: 2, auto: true };
        let t0 = Instant::now();
        let mut t = Timer::focus(&cfg, t0);
        assert_eq!(t.short(t0), "25m");
        t.toggle_pause(t0 + Duration::from_secs(60));
        assert_eq!(t.left(t0 + Duration::from_secs(600)), Duration::from_secs(24 * 60), "paused time doesn't run");
        assert_eq!(t.next_change(t0, false), None);
        t.toggle_pause(t0 + Duration::from_secs(600));
        assert!(!t.done(t0 + Duration::from_secs(600 + 24 * 60 - 1)));
        assert!(t.done(t0 + Duration::from_secs(600 + 24 * 60)));
        let phases: Vec<(Phase, u32)> = std::iter::successors(Some(t), |t| t.next(&cfg, t0)).take(5).map(|t| (t.phase, t.round)).collect();
        assert_eq!(phases, [(Phase::Focus, 1), (Phase::Short, 1), (Phase::Focus, 2), (Phase::Long, 2), (Phase::Focus, 1)]);
        assert_eq!(Timer::new(Phase::Countdown, 1, mins(1), t0).next(&cfg, t0), None);
    }

    #[test]
    fn wakes_once_a_minute_until_the_last_one() {
        let t0 = Instant::now();
        let t = Timer::new(Phase::Countdown, 1, Duration::from_secs(150), t0);
        assert_eq!(t.next_change(t0, false), Some(t0 + Duration::from_secs(30)), "150s -> 120s shows 2m");
        assert_eq!(t.next_change(t0 + Duration::from_secs(100), false), Some(t0 + Duration::from_secs(101)));
        assert_eq!(t.next_change(t0, true), Some(t0 + Duration::from_secs(1)));
        assert_eq!(t.short(t0 + Duration::from_secs(100)), "0:50");
        assert_eq!(t.clock(t0), "2:30");
    }
}
