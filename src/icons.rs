//! Real application icons: app id -> .desktop entry -> Icon= -> file -> raster.
//! gdk-pixbuf would now route every decode through glycin's sandbox, so PNG goes
//! through cairo, SVG through resvg and JPEG (album art) through zune-jpeg.
use cairo::{Context, Filter, Format, ImageSurface, SurfacePattern};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SIZES: [&str; 14] = ["scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64",
    "48x48", "36x36", "32x32", "24x24", "22x22", "16x16"];
const RESCAN: Duration = Duration::from_secs(30);

fn data_dirs() -> Vec<PathBuf> {
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
    let tree = resvg::usvg::Tree::from_data(&fs::read(path).ok()?, &Default::default()).ok()?;
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
    symbols: HashMap<(&'static str, i32), Option<ImageSurface>>,
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
        Icons { index: scan_desktop_files(&dirs), scanned: Instant::now(), dirs, themes,
                apps: HashMap::new(), symbols: HashMap::new() }
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

    /// Adwaita symbolic icon as a mask, for painting in any palette color.
    pub fn symbol(&mut self, name: &'static str, px: i32) -> Option<ImageSurface> {
        self.symbols.entry((name, px)).or_insert_with(|| {
            let root = Path::new("/usr/share/icons/Adwaita/symbolic");
            let file = format!("{name}-symbolic.svg");
            let hit = fs::read_dir(root).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.exists())?;
            load_svg(&hit, px)
        }).clone()
    }

    pub fn tray(&mut self, icon: &crate::bus::TrayIcon, px: i32) -> Option<ImageSurface> {
        use crate::bus::TrayIcon;
        match icon {
            TrayIcon::None => None,
            TrayIcon::Pixels { w, h, rgba } => fit(&from_rgba(rgba, *w, *h)?, px, false),
            TrayIcon::Named { name, theme_path } => {
                let own = theme_path.as_ref().and_then(|t| {
                    let root = Path::new(t);
                    let dirs = [root.to_owned(), root.join("hicolor")];
                    ["svg", "png"].iter().find_map(|e| {
                        let flat = root.join(format!("{name}.{e}"));
                        flat.exists().then_some(flat)
                    }).or_else(|| find_file(name, &dirs[..1], &["hicolor".into()]))
                });
                let path = own.or_else(|| find_file(name, &self.dirs, &self.themes))?;
                load(&path, px)
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
}
