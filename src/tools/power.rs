//! Lock, log out, suspend, reboot, shut down. Anything that would lose open work
//! asks for a second Enter first.
use super::{Act, Ctx, Icon, Item};

pub fn list(ctx: &Ctx) -> Vec<Item> {
    let confirm = |text: &str, act| Act::Confirm(text.to_owned(), Box::new(act));
    let sh = |s: &str| Act::Sh(s.to_owned());
    // systemd has these in systemctl; elogind (OpenRC, Gentoo/Artix/Void) only in loginctl.
    let power = |verb: &str| sh(&format!("if command -v systemctl >/dev/null; then systemctl {verb}; else loginctl {verb}; fi"));
    let row = |title, glyph: &str, act| Item::new(title, Icon::Glyph(glyph.into()), act);
    vec![
        row("Lock", "lock", Act::Sh(ctx.cfg.commands.lock.clone())),
        row("Suspend", "bedtime", power("suspend")),
        row("Log out", "logout", confirm("Press Enter again to log out", Act::Quit)),
        row("Reboot", "restart_alt", confirm("Press Enter again to reboot", power("reboot"))),
        row("Shut down", "power_settings_new", confirm("Press Enter again to shut down", power("poweroff"))),
    ]
}
