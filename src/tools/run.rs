//! A run prompt: every executable on PATH, or whatever you typed.
use super::{Act, Ctx, Icon, Item, quote};
use std::{collections::BTreeSet, env, fs, os::unix::fs::PermissionsExt};

pub fn list(_: &Ctx) -> Vec<Item> {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut names = BTreeSet::new();
    for dir in env::split_paths(&path) {
        for file in fs::read_dir(dir).into_iter().flatten().flatten() {
            let runnable = file.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
            if runnable {
                names.insert(file.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names.into_iter().map(|n| {
        let act = Act::Sh(quote(&n));
        Item::new(n, Icon::None, act)
    }).collect()
}

pub fn typed(query: &str) -> Act {
    Act::Sh(query.to_owned())
}
