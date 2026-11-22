//! Changes to config.toml from Settings. The file is the owner's: their comments,
//! order and spacing stay, a change that doesn't parse is never written, and what
//! was there before the last change is beside it as config.toml.bak.
use crate::settings::Config;
use serde_json::Value as Json;
use std::{fs, path::Path};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, TableLike, Value};

/// A path to a setting and what to make it. None takes the line out, for the default.
pub type Change = (Vec<String>, Option<Json>);

/// A dotted key as a path. Only for the island's own keys: an app id has dots in it.
pub fn path(key: &str) -> Vec<String> {
    key.split('.').map(str::to_owned).collect()
}

fn value(v: &Json) -> Result<Value, String> {
    Ok(match v {
        Json::String(s) => Value::from(s.as_str()),
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Value::from(i),
            (None, Some(f)) if f.is_finite() => Value::from(f),
            _ => return Err(format!("{n} isn't a number config.toml can hold")),
        },
        Json::Array(items) => {
            let mut out = Array::new();
            for item in items {
                out.push(value(item)?);
            }
            Value::Array(out)
        }
        _ => return Err("that isn't a value config.toml can hold".into()),
    })
}

fn prefix(v: &Value) -> &str {
    v.decor().prefix().and_then(|p| p.as_str()).unwrap_or_default()
}

/// A list that was laid out a line to an entry stays that way, comments and all.
fn relist(old: &Array, new: &Array) -> Array {
    let tail = old.trailing().as_str().unwrap_or_default();
    if !tail.contains('\n') && !old.iter().any(|v| prefix(v).contains('\n')) {
        return new.clone();
    }
    let mut left: Vec<&Value> = old.iter().collect();
    let mut out = Array::new();
    for item in new.iter() {
        let same = left.iter().position(|o| o.as_str().is_some() && o.as_str() == item.as_str());
        let mut kept = same.map_or_else(|| item.clone(), |at| left.remove(at).clone());
        if !prefix(&kept).contains('\n') {
            let lead = format!("\n    {}", prefix(&kept).trim_start());
            kept.decor_mut().set_prefix(lead);
        }
        out.push_formatted(kept);
    }
    // What was said beside an entry that's gone moves to the end rather than going with it.
    let said: String = left.into_iter().filter(|v| prefix(v).contains('#')).map(|v| prefix(v).trim_end()).collect();
    out.set_trailing(format!("{said}{}", if tail.contains('\n') { tail } else { "\n" }));
    out.set_trailing_comma(true);
    out
}

fn set_tables(table: &mut dyn TableLike, name: &str, index: usize, rest: &[String], new: Option<&Json>) -> Result<(), String> {
    if !table.contains_key(name) {
        table.insert(name, Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let list = table.get_mut(name).and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| format!("{name} is written inline in config.toml: edit it there"))?;
    match (rest, new) {
        ([], Some(Json::Object(fields))) if index == list.len() => {
            let mut entry = Table::new();
            for (key, v) in fields {
                entry.insert(key, Item::Value(value(v)?));
            }
            list.push(entry);
        }
        ([], None) if index < list.len() => {
            list.remove(index);
            if list.is_empty() {
                table.remove(name);
            }
        }
        ([_, ..], _) if index < list.len() => {
            let entry = list.get_mut(index).ok_or("that entry is gone")?;
            return set(entry, rest, new);
        }
        _ => return Err(format!("that {name} entry is no longer there")),
    }
    Ok(())
}

fn set(table: &mut dyn TableLike, key: &[String], new: Option<&Json>) -> Result<(), String> {
    let Some((name, rest)) = key.split_first() else { return Err("no setting named".into()) };
    if let Some((index, rest)) = rest.split_first().and_then(|(i, r)| Some((i.parse::<usize>().ok()?, r))) {
        return set_tables(table, name, index, rest, new);
    }
    if !rest.is_empty() {
        if !table.contains_key(name) {
            if new.is_none() {
                return Ok(());
            }
            table.insert(name, Item::Table(Table::new()));
        }
        let inner = table.get_mut(name).and_then(Item::as_table_like_mut).ok_or_else(|| format!("{name} isn't a section"))?;
        return set(inner, rest, new);
    }
    let Some(new) = new else {
        table.remove(name);
        return Ok(());
    };
    let mut fresh = value(new)?;
    match table.get_mut(name) {
        Some(Item::Value(old)) => {
            if let (Some(was), Some(now)) = (old.as_array(), fresh.as_array()) {
                fresh = Value::Array(relist(was, now));
            }
            // The comment after it on the line is part of its decor.
            *fresh.decor_mut() = old.decor().clone();
            *old = fresh;
        }
        Some(Item::None) | None => {
            table.insert(name, Item::Value(fresh));
        }
        Some(_) => return Err(format!("{name} is a section in config.toml")),
    }
    Ok(())
}

/// The text with the changes made, checked to still be a config.
fn edited(src: &str, changes: &[Change]) -> Result<String, String> {
    let first_line = |e: &dyn std::fmt::Display| e.to_string().lines().next().unwrap_or_default().to_owned();
    toml::from_str::<Config>(src).map_err(|e| format!("config.toml has an error, so it's untouched: {}", first_line(&e)))?;
    let mut doc: DocumentMut = src.parse().map_err(|e| format!("config.toml doesn't read: {}", first_line(&e)))?;
    for (key, new) in changes {
        set(doc.as_table_mut(), key, new.as_ref())?;
    }
    let out = doc.to_string();
    toml::from_str::<Config>(&out).map_err(|e| format!("not saved, the island wouldn't take it: {}", first_line(&e)))?;
    Ok(out)
}

pub fn write(changes: &[Change]) -> Result<(), String> {
    write_at(&Config::path(), changes)
}

fn write_at(path: &Path, changes: &[Change]) -> Result<(), String> {
    // A dotfiles link is written through, never replaced.
    let file = match fs::canonicalize(path) {
        Ok(real) => real,
        Err(_) if fs::symlink_metadata(path).is_ok() => return Err("config.toml is a link to a file that isn't there".into()),
        Err(_) => path.to_owned(),
    };
    let before = match fs::read_to_string(&file) {
        Ok(src) => Some(src),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read config.toml: {e}")),
    };
    let after = edited(before.as_deref().unwrap_or_default(), changes)?;
    if before.as_deref() == Some(after.as_str()) {
        return Ok(());
    }
    let failed = |e: std::io::Error| format!("can't write config.toml: {e}");
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(failed)?;
    }
    let tmp = file.with_extension("toml.new");
    if before.is_some() {
        fs::copy(&file, file.with_extension("toml.bak")).map_err(failed)?;
        // Copied first, so the new file has the old one's permissions.
        fs::copy(&file, &tmp).map_err(failed)?;
    }
    fs::write(&tmp, after).and_then(|()| fs::File::open(&tmp)?.sync_all()).map_err(failed)?;
    fs::rename(&tmp, &file).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edits_leave_the_rest_of_the_file_as_it_was() {
        let src = "# mine\nfont = \"Inter\"   # the good one\npill = [\n    \"media\", # art first\n    \"clock\",\n]\n\n[ui]\n# bigger\nscale = 1.0\n";
        let out = edited(src, &[
            (path("font"), Some(json!("Rubik"))),
            (path("ui.scale"), Some(json!(1.25))),
            (path("pill"), Some(json!(["clock", "media", "battery"]))),
            (path("notify.low"), Some(json!(4))),
        ]).unwrap();
        for kept in ["# mine", "font = \"Rubik\"   # the good one", "# art first", "# bigger\nscale = 1.25", "[notify]"] {
            assert!(out.contains(kept), "lost {kept:?} in\n{out}");
        }
        let c = toml::from_str::<Config>(&out).unwrap();
        assert_eq!((c.pill, c.notify.low), (vec!["clock".to_owned(), "media".into(), "battery".into()], 4));
        let gone = edited(&out, &[(path("pill"), Some(json!(["battery"])))]).unwrap();
        assert!(gone.contains("# art first"), "a removed entry's comment stays:\n{gone}");
        assert_eq!(toml::from_str::<Config>(&gone).unwrap().pill, ["battery"]);
    }

    #[test]
    fn tables_in_lists_and_app_ids_with_dots() {
        let mut out = edited("", &[(path("custom.0"), Some(json!({"name": "w", "exec": ""})))]).unwrap();
        out = edited(&out, &[(path("custom.0.exec"), Some(json!("date"))),
                             (vec!["icons".into(), "org.gnome.Nautilus".into()], Some(json!("folder")))]).unwrap();
        let c = toml::from_str::<Config>(&out).unwrap();
        assert_eq!((c.custom[0].exec.as_str(), c.icons["org.gnome.Nautilus"].as_str()), ("date", "folder"));
        let out = edited(&out, &[(path("custom.0"), None)]).unwrap();
        assert!(toml::from_str::<Config>(&out).unwrap().custom.is_empty());
        assert!(edited(&out, &[(path("custom.3.exec"), Some(json!("x")))]).is_err());
    }

    #[test]
    fn a_broken_file_or_value_is_never_written() {
        assert!(edited("font = ", &[(path("font"), Some(json!("x")))]).is_err());
        assert!(edited("", &[(path("style"), Some(json!("blob")))]).is_err());
        assert!(edited("", &[(path("ui.scale"), Some(json!("large")))]).is_err());
        assert_eq!(edited("# only this\n", &[(path("ui.scale"), None)]).unwrap(), "# only this\n");
    }

    #[test]
    fn a_link_is_written_through_and_backed_up() {
        let dir = std::env::temp_dir().join(format!("island-edit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (real, link) = (dir.join("real.toml"), dir.join("config.toml"));
        fs::write(&real, "font = \"Inter\" # keep\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_at(&link, &[(path("font"), Some(json!("Rubik")))]).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "still a link");
        assert_eq!(fs::read_to_string(&real).unwrap(), "font = \"Rubik\" # keep\n");
        assert_eq!(fs::read_to_string(dir.join("real.toml.bak")).unwrap(), "font = \"Inter\" # keep\n");
        fs::remove_dir_all(dir).unwrap();
    }
}
