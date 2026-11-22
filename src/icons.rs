//! Real application icons: app id -> .desktop entry -> Icon= -> file -> raster.
//! gdk-pixbuf would now route every decode through glycin's sandbox, so PNG goes
//! through cairo, SVG through resvg and JPEG (album art) through zune-jpeg.
use crate::symbols::{FILLED, SYMBOLS};
use cairo::{Context, Filter, Format, ImageSurface, SurfacePattern};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SIZES: [&str; 14] = ["scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64",
    "48x48", "36x36", "32x32", "24x24", "22x22", "16x16"];
/// Trimmed off each side of an icon's box.
const MARGIN: i32 = 16;
const RESCAN: Duration = Duration::from_secs(30);
/// Line icons, which have nothing to fill, go from weight 500 to about 800.
const LINE: u32 = 40;
/// Filled ones only have their corners softened: more would close what's cut out of them.
const SOFT: u32 = 12;
/// How much of a level icon's empty part shows.
const REST: f64 = 0.36;

pub fn data_dirs() -> Vec<PathBuf> {
    let home = crate::settings::home();
    let mut dirs = vec![env::var_os("XDG_DATA_HOME").map_or(home.join(".local/share"), PathBuf::from)];
    let sys = env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    dirs.push(home.join(".local/share/flatpak/exports/share"));
    dirs.push("/var/lib/flatpak/exports/share".into());
    let mut seen = Vec::new();
    dirs.retain(|d| !seen.contains(d) && { seen.push(d.clone()); true });
    dirs
}

/// Adwaita's symbolic icons, only a fallback for tray items that name one: /usr/share on most
/// distros, somewhere under XDG_DATA_DIRS on NixOS.
pub fn adwaita_symbolic() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| data_dirs().into_iter().map(|d| d.join("icons/Adwaita/symbolic")).find(|p| p.is_dir())).clone()
}

/// The table's own copy of a name, if it's one of the island's icons.
pub fn builtin(name: &str) -> Option<&'static str> {
    entry(name).map(|e| e.0)
}

fn entry(name: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    SYMBOLS.binary_search_by(|(n, ..)| (*n).cmp(name)).ok().map(|i| &SYMBOLS[i])
}

fn builtin_svg(name: &str, fill: bool, margin: i32) -> Option<String> {
    let (_, outline, _) = *entry(name)?;
    let (x, y, w) = (margin, margin - 960, 960 - 2 * margin);
    let svg = |body: String| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x} {y} {w} {w}">{body}</svg>"#);
    if !fill {
        return Some(svg(format!(r#"<path d="{outline}"/>"#)));
    }
    let path = |d: &str, width: u32, more: &str| {
        format!(r##"<path d="{d}" stroke="#000" stroke-width="{width}" stroke-linejoin="round"{more}/>"##)
    };
    Some(svg(match level(name) {
        // The whole body, dim, with what's in it at full strength on top.
        Some((body, reach)) => {
            let body = entry(body)?.1;
            format!(r#"<clipPath id="l">{reach}</clipPath><g opacity="{REST}">{}</g>{}"#,
                    path(body, SOFT, ""), path(body, SOFT, r##" clip-path="url(#l)""##))
        }
        None => match filled(name) {
            Some(d) => path(d, SOFT, ""),
            None => path(outline, LINE, ""),
        },
    }))
}

/// The filled path, where there's something to fill.
fn filled(name: &str) -> Option<&'static str> {
    if name == "wifi" {
        // The wedge the signal levels are drawn in, full.
        return entry("signal_wifi_4_bar").map(|e| e.1);
    }
    let ours = FILLED.iter().find(|(n, _)| *n == name).map(|f| f.1);
    ours.or_else(|| entry(name).map(|e| e.2).filter(|d| !d.is_empty()))
}

/// An icon that says how much: the body it's a part of, and the part that's full.
fn level(name: &str) -> Option<(&'static str, String)> {
    let battery = |bars: i32| {
        let h = 110 + bars * 100;
        ("battery_full", format!(r#"<rect x="0" y="{}" width="960" height="{h}"/>"#, -60 - h))
    };
    let wifi = |r: i32| ("signal_wifi_4_bar", format!(r#"<circle cx="480" cy="-140" r="{r}"/>"#));
    Some(match name {
        "battery_1_bar" => battery(1),
        "battery_2_bar" => battery(2),
        "battery_3_bar" => battery(3),
        "battery_4_bar" => battery(4),
        "battery_5_bar" => battery(5),
        "battery_6_bar" => battery(6),
        "signal_wifi_0_bar" => wifi(1),
        "network_wifi_1_bar" => wifi(250),
        "network_wifi_2_bar" => wifi(370),
        "network_wifi_3_bar" => wifi(480),
        _ => return None,
    })
}

/// Map every name a window might use (file stem, its last dotted part,
/// StartupWMClass) to the entry's Icon=. First data dir wins, as in the spec.
fn scan_desktop_files(dirs: &[PathBuf]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for dir in dirs {
        for entry in fs::read_dir(dir.join("applications")).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Ok(src) = fs::read_to_string(&path) else { continue };
            let mut in_entry = false;
            let (mut icon, mut class) = (None, None);
            for line in src.lines() {
                if line.starts_with('[') {
                    in_entry = line == "[Desktop Entry]";
                } else if in_entry {
                    if let Some(v) = line.strip_prefix("Icon=") {
                        icon = Some(v.trim().to_owned());
                    } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
                        class = Some(v.trim().to_lowercase());
                    }
                }
            }
            let Some(icon) = icon else { continue };
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
            let tail = stem.rsplit('.').next().unwrap_or(&stem).to_owned();
            for key in [Some(stem.clone()), Some(tail), class].into_iter().flatten() {
                index.entry(key).or_insert_with(|| icon.clone());
            }
        }
    }
    index
}

fn configured_theme() -> Option<String> {
    let src = fs::read_to_string(crate::settings::home().join(".config/gtk-3.0/settings.ini")).ok()?;
    let line = src.lines().find_map(|l| l.strip_prefix("gtk-icon-theme-name="))?;
    Some(line.trim().to_owned())
}

/// Pick the file for an Icon= value. Best size first: rasters only ever shrink.
fn find_file(name: &str, dirs: &[PathBuf], themes: &[String]) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_absolute() {
        return direct.exists().then(|| direct.to_owned());
    }
    let hit = |p: PathBuf| ["svg", "png"].iter().map(|e| p.with_extension(e)).find(|p| p.exists());
    // with_extension would eat a dotted name's tail, so build the name by hand.
    let file = format!("{name}.x");
    for theme in themes {
        for dir in dirs {
            let root = dir.join("icons").join(theme);
            if !root.is_dir() {
                continue;
            }
            for size in SIZES {
                if let Some(p) = hit(root.join(size).join("apps").join(&file)) {
                    return Some(p);
                }
            }
            // breeze keeps sizes below the context: apps/48/name.svg
            for size in ["48", "64", "32", "22", "16"] {
                if let Some(p) = hit(root.join("apps").join(size).join(&file)) {
                    return Some(p);
                }
            }
        }
    }
    dirs.iter().find_map(|d| hit(d.join("pixmaps").join(&file)))
}

pub fn from_rgba(rgba: &[u8], w: i32, h: i32) -> Option<ImageSurface> {
    let mut surf = ImageSurface::create(Format::ARgb32, w, h).ok()?;
    let stride = surf.stride() as usize;
    {
        let mut d = surf.data().ok()?;
        for (y, row) in rgba.chunks_exact(w as usize * 4).enumerate() {
            let out = &mut d[y * stride..][..w as usize * 4];
            for (o, p) in out.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                o.copy_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
    }
    Some(surf)
}

fn fit(src: &ImageSurface, px: i32, cover: bool) -> Option<ImageSurface> {
    let out = ImageSurface::create(Format::ARgb32, px, px).ok()?;
    let (sw, sh) = (src.width() as f64, src.height() as f64);
    let s = if cover { (px as f64 / sw).max(px as f64 / sh) } else { (px as f64 / sw).min(px as f64 / sh) };
    let cr = Context::new(&out).ok()?;
    cr.translate((px as f64 - sw * s) / 2.0, (px as f64 - sh * s) / 2.0);
    cr.scale(s, s);
    let pat = SurfacePattern::create(src);
    pat.set_filter(Filter::Best);
    cr.set_source(&pat).ok()?;
    cr.paint().ok()?;
    drop(cr);
    Some(out)
}

fn load_svg(path: &Path, px: i32) -> Option<ImageSurface> {
    render_svg(&fs::read(path).ok()?, px)
}

fn render_svg(svg: &[u8], px: i32) -> Option<ImageSurface> {
    let tree = resvg::usvg::Tree::from_data(svg, &Default::default()).ok()?;
    let size = tree.size();
    let s = px as f32 / size.width().max(size.height());
    let (dx, dy) = ((px as f32 - size.width() * s) / 2.0, (px as f32 - size.height() * s) / 2.0);
    let mut pm = resvg::tiny_skia::Pixmap::new(px as u32, px as u32)?;
    let ts = resvg::tiny_skia::Transform::from_scale(s, s).post_translate(dx, dy);
    resvg::render(&tree, ts, &mut pm.as_mut());
    from_rgba(pm.data(), px, px)
}

fn load_png(path: &Path, px: i32, cover: bool) -> Option<ImageSurface> {
    let src = ImageSurface::create_from_png(&mut fs::File::open(path).ok()?).ok()?;
    fit(&src, px, cover)
}

fn load_jpeg(path: &Path, px: i32) -> Option<ImageSurface> {
    use zune_jpeg::{JpegDecoder, zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions}};
    let bytes = fs::read(path).ok()?;
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(bytes), opts);
    let rgba = dec.decode().ok()?;
    let (w, h) = dec.dimensions()?;
    fit(&from_rgba(&rgba, w as i32, h as i32)?, px, true)
}

/// Album art comes as PNG or JPEG and is cropped to fill its tile. Sniff the
/// bytes: cached downloads have no extension.
pub fn load_art(path: &Path, px: i32) -> Option<ImageSurface> {
    let mut magic = [0u8; 2];
    std::io::Read::read_exact(&mut fs::File::open(path).ok()?, &mut magic).ok()?;
    if magic == [0xff, 0xd8] { load_jpeg(path, px) } else { load_png(path, px, true) }
}

fn load(path: &Path, px: i32) -> Option<ImageSurface> {
    if path.extension().is_some_and(|e| e == "svg") { load_svg(path, px) } else { load_png(path, px, false) }
}

pub struct Icons {
    dirs: Vec<PathBuf>,
    themes: Vec<String>,
    index: HashMap<String, String>,
    scanned: Instant,
    apps: HashMap<(String, i32), Option<ImageSurface>>,
    /// Palette rows and notification images, by source. Emptied when the palette closes.
    files: HashMap<(String, i32), Option<ImageSurface>>,
    symbols: HashMap<(&'static str, bool, i32), Option<ImageSurface>>,
}

impl Icons {
    pub fn new() -> Self {
        let dirs = data_dirs();
        let mut themes: Vec<String> = configured_theme().into_iter().collect();
        for t in ["hicolor", "Adwaita", "breeze", "breeze-dark"] {
            if !themes.iter().any(|x| x == t) {
                themes.push(t.into());
            }
        }
        Icons { index: HashMap::new(), scanned: Instant::now() - RESCAN, dirs, themes,
                apps: HashMap::new(), files: HashMap::new(), symbols: HashMap::new() }
    }

    fn lookup(&mut self, appid: &str) -> Option<PathBuf> {
        let id = appid.to_lowercase();
        // Proton windows: `steam_app_N`, whose icon Steam installs as steam_icon_N.
        let steam = id.strip_prefix("steam_app_").map(|n| format!("steam_icon_{n}"));
        let tail = id.rsplit('.').next().unwrap_or(&id).to_owned();
        let found = |s: &Self| {
            let named = s.index.get(&id).or_else(|| s.index.get(&tail)).map(String::as_str);
            let names = [steam.as_deref(), named, Some(appid), Some(&id), steam.as_ref().map(|_| "steam")];
            names.into_iter().flatten().find_map(|n| find_file(n, &s.dirs, &s.themes))
        };
        // Apps installed since the last scan miss first time round.
        found(self).or_else(|| {
            if self.scanned.elapsed() < RESCAN {
                return None;
            }
            self.index = scan_desktop_files(&self.dirs);
            self.scanned = Instant::now();
            found(self)
        })
    }

    /// None means nothing matched: the caller draws a lettered tile.
    pub fn app(&mut self, appid: &str, px: i32) -> Option<ImageSurface> {
        let key = (appid.to_owned(), px);
        if let Some(hit) = self.apps.get(&key) {
            return hit.clone();
        }
        let icon = self.lookup(appid).and_then(|p| load(&p, px));
        // A miss isn't cached until the index has been refreshed since the app
        // appeared, or an app installed moments ago would stay lettered forever.
        if icon.is_some() || self.scanned.elapsed() < RESCAN {
            self.apps.insert(key, icon.clone());
        }
        icon
    }

    /// An Icon= value: a theme name or a path.
    pub fn themed(&mut self, name: &str, px: i32) -> Option<ImageSurface> {
        let (dirs, themes) = (&self.dirs, &self.themes);
        self.files.entry((name.to_owned(), px)).or_insert_with(|| load(&find_file(name, dirs, themes)?, px)).clone()
    }

    /// An image file of any kind we can read, cropped to a square.
    pub fn picture(&mut self, path: &Path, px: i32) -> Option<ImageSurface> {
        let key = (path.to_string_lossy().into_owned(), px);
        self.files.entry(key).or_insert_with(|| {
            if path.extension().is_some_and(|e| e == "svg") { load_svg(path, px) } else { load_art(path, px) }
        }).clone()
    }

    pub fn pixels(&self, rgba: &[u8], w: i32, h: i32, px: i32) -> Option<ImageSurface> {
        fit(&from_rgba(rgba, w, h)?, px, true)
    }

    /// Give back what a closed palette loaded.
    pub fn forget_files(&mut self) {
        self.files.clear();
    }

    /// One of the island's icons as a mask, for painting in any palette color.
    pub fn symbol(&mut self, name: &'static str, fill: bool, px: i32) -> Option<ImageSurface> {
        self.symbols.entry((name, fill, px)).or_insert_with(|| render_svg(builtin_svg(name, fill, MARGIN)?.as_bytes(), px)).clone()
    }

    /// Returns the raster, and whether it's a mask to paint in the theme color.
    pub fn tray(&mut self, icon: &crate::bus::TrayIcon, px: i32) -> Option<(ImageSurface, bool)> {
        use crate::bus::TrayIcon;
        match icon {
            TrayIcon::None => None,
            TrayIcon::Pixels { w, h, rgba } => Some((fit(&from_rgba(rgba, *w, *h)?, px, false)?, false)),
            TrayIcon::Named { name, theme_path } => {
                let own = theme_path.as_ref().and_then(|t| {
                    let root = Path::new(t);
                    ["svg", "png"].iter().map(|e| root.join(format!("{name}.{e}"))).find(|p| p.exists())
                        .or_else(|| find_file(name, &[root.to_owned()], &["hicolor".into()]))
                });
                if let Some(path) = own.or_else(|| find_file(name, &self.dirs, &self.themes)) {
                    return Some((load(&path, px)?, name.ends_with("-symbolic")));
                }
                let file = format!("{}.svg", name.strip_suffix("-symbolic").unwrap_or(name));
                let file = format!("{}-symbolic.svg", file.trim_end_matches(".svg"));
                let root = adwaita_symbolic()?;
                let hit = fs::read_dir(root).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.exists())?;
                Some((load_svg(&hit, px)?, true))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_ids_through_desktop_entries_and_keeps_dotted_names() {
        let root = env::temp_dir().join(format!("island-icons-{}", std::process::id()));
        let apps = root.join("applications");
        let icons = root.join("icons/hicolor/48x48/apps");
        fs::create_dir_all(&apps).unwrap();
        fs::create_dir_all(&icons).unwrap();
        fs::write(apps.join("org.example.Tool.desktop"),
                  "[Desktop Entry]\nIcon=org.example.tool.fancy\nStartupWMClass=ToolWin\n[Desktop Action x]\nIcon=wrong\n").unwrap();
        fs::write(icons.join("org.example.tool.fancy.png"), b"x").unwrap();

        let dirs = vec![root.clone()];
        let index = scan_desktop_files(&dirs);
        for key in ["org.example.tool", "tool", "toolwin"] {
            assert_eq!(index[key], "org.example.tool.fancy");
        }
        let hit = find_file(&index["tool"], &dirs, &["hicolor".into()]).unwrap();
        assert!(hit.ends_with("org.example.tool.fancy.png"), "{hit:?}");
        assert!(find_file("nothing", &dirs, &["hicolor".into()]).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_builtin_icon_renders_both_ways_and_isnt_cropped() {
        assert!(SYMBOLS.windows(2).all(|w| w[0].0 < w[1].0), "lookup needs a sorted table");
        let ink = |svg: String| -> u64 {
            let mut s = render_svg(svg.as_bytes(), 384).expect("parses");
            s.data().unwrap().chunks(4).map(|p| p[3] as u64).sum()
        };
        for (name, ..) in SYMBOLS {
            for fill in [false, true] {
                let full = ink(builtin_svg(name, fill, 0).unwrap());
                assert!(full > 0, "{name} is blank");
                // A smaller box draws it larger by this much; less means an edge cut it off.
                let grown = (960.0 / (960 - 2 * MARGIN) as f64).powi(2);
                let trimmed = ink(builtin_svg(name, fill, MARGIN).unwrap()) as f64;
                assert!(trimmed >= full as f64 * grown * 0.97, "{name} is cropped");
            }
        }
        assert_eq!(builtin("keep"), Some("keep"));
        assert_eq!(builtin("\u{f0349}"), None, "a font glyph stays a glyph");
    }

    /// Solid that only thickens an outline is how terminal, clipboard and window stayed hollow.
    #[test]
    fn solid_means_filled() {
        // Arrows, chevrons, ticks and marks: lines, or shapes that are already all ink.
        const ALL_INK: &[&str] = &["arrow_back", "arrow_forward", "battery_full", "bluetooth", "bluetooth_disabled",
            "bluetooth_searching", "check", "chevron_left", "chevron_right", "close", "close_fullscreen", "collapse_content",
            "crop", "graphic_eq", "keyboard_arrow_down", "keyboard_arrow_left", "keyboard_arrow_right", "keyboard_arrow_up",
            "music_note", "restart_alt", "signal_wifi_4_bar"];
        for (name, outline, _) in SYMBOLS {
            let solid = filled(name).is_some_and(|d| d != *outline);
            assert!(solid || level(name).is_some() || ALL_INK.contains(name), "{name} is hollow when solid");
        }
        for (name, _) in FILLED {
            assert!(entry(name).is_some(), "{name} fills no icon");
        }
    }

    /// A name that isn't in the table draws nothing, silently: catch typos.
    #[test]
    fn icon_names_in_the_source_exist() {
        let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules");
        for file in fs::read_dir(modules).unwrap().flatten() {
            let src = fs::read_to_string(file.path()).unwrap();
            for call in ["symbol(\"", "button(\"", "icon(\""] {
                for rest in src.split(call).skip(1) {
                    let name = rest.split('"').next().unwrap();
                    assert!(builtin(name).is_some(), "{:?}: no icon called {name}", file.path());
                }
            }
        }
    }
}
