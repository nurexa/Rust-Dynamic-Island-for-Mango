//! The drawing kit modules share, and the types they hand back. Both island
//! states render once into a cached surface with their click targets, and the
//! frame loop just composites those. Nothing here runs per animation frame.
use crate::{
    bus::MenuEntry,
    icons::Icons,
    settings::{Config, IconStyle},
    theme::{Palette, Rgb, mix},
};
use cairo::{Context, Format, ImageSurface};
use pango::{AttrFontFeatures, AttrList, EllipsizeMode, FontDescription, Weight};

pub type Rect = (f64, f64, f64, f64);

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    View { tag: u32, mon: String },
    MoveTo(u32),
    Focus(u64),
    Sh(String),
    Mute,
    Volume,
    Pin,
    Reset,
    Tuck,
    TrayClick(usize),
    TrayRight(usize),
    TrayMiddle(usize),
    MenuClick(i32),
    MenuSub(i32),
    MenuBack,
    Open(&'static str),
    PaletteRow(usize),
    PaletteTool(usize),
    NoteAction(u32, String),
    NoteDismiss(u32),
    NoteClear,
    Dnd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    Tags,
    Volume,
    Palette,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub rect: Rect,
    pub radius: f64,
    pub left: Action,
    pub right: Action,
    pub middle: Action,
    pub scroll: Option<Scroll>,
    pub tip: Option<String>,
}

impl Region {
    pub fn new(rect: Rect, radius: f64, left: Action) -> Self {
        Region { rect, radius, left, right: Action::None, middle: Action::None, scroll: None, tip: None }
    }
    pub fn right(mut self, a: Action) -> Self {
        self.right = a;
        self
    }
    pub fn middle(mut self, a: Action) -> Self {
        self.middle = a;
        self
    }
    pub fn tip(mut self, t: impl Into<String>) -> Self {
        self.tip = Some(t.into());
        self
    }
}

pub struct Content {
    pub surf: ImageSurface,
    pub w: f64,
    pub h: f64,
    pub regions: Vec<Region>,
}

/// What a panel is showing: the system view, or a tray app's menu.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Main,
    Menu { item: usize, title: String, entries: Vec<MenuEntry>, root: Vec<MenuEntry> },
    Palette,
    Toast,
}

/// App id -> glyph. The ids match what Mango reports, lowercased; a dotted id
/// also matches on its last part, so `org.foo.kitty` finds `kitty`.
const GLYPHS: &[(&str, &str)] = &[
    ("alacritty", "󰆍"),
    ("blender", "󰂫"),
    ("brave-browser", "󰊯"),
    ("chromium", "󰊯"),
    ("code", "󰨞"),
    ("code-oss", "󰨞"),
    ("com.obsproject.studio", "󰑋"),
    ("discord", "󰙯"),
    ("dolphin", "󰉋"),
    ("firefox", "󰈹"),
    ("foot", "󰆍"),
    ("ghostty", "󰆍"),
    ("google-chrome", "󰊯"),
    ("kitty", "󰆍"),
    ("konsole", "󰆍"),
    ("librewolf", "󰈹"),
    ("mpv", "󰐹"),
    ("nyachan", "󰊴"),
    ("obsidian", "󰠮"),
    ("org.kde.dolphin", "󰉋"),
    ("org.prismlauncher.prismlauncher", "󰍳"),
    ("org.vinegarhq.sober", "󰊴"),
    ("org.vinegarhq.vinegar", "󰊴"),
    ("pavucontrol", "󰕾"),
    ("resolve", "󰕧"),
    ("spotify", "󰓇"),
    ("steam", "󰓓"),
    ("thorium-browser", "󰊯"),
    ("vesktop", "󰙯"),
    ("vlc", "󰕼"),
    ("wezterm", "󰆍"),
    ("wootility", "󰌌"),
    ("zen", "󰈹"),
    ("zen-browser", "󰈹"),
];
const GENERIC_GLYPH: &str = "󰣆";
const GAME_GLYPH: &str = "󰊴";

pub fn glyph_for<'a>(cfg: &'a Config, appid: &str) -> &'a str {
    let id = appid.to_lowercase();
    let tail = id.rsplit('.').next().unwrap_or(&id);
    if let Some(g) = cfg.icons.get(&id).or_else(|| cfg.icons.get(tail)) {
        return g;
    }
    if id.starts_with("steam_app_") {
        return GAME_GLYPH;
    }
    let known = |key: &str| GLYPHS.iter().find(|(k, _)| *k == key).map(|(_, g)| *g);
    known(&id).or_else(|| known(tail)).unwrap_or(GENERIC_GLYPH)
}

pub struct Pen<'a> {
    pub cr: Context,
    layout: pango::Layout,
    cfg: &'a Config,
    pub pal: &'a Palette,
    pub scale: i32,
    pub icons: &'a mut Icons,
    pub regions: Vec<Region>,
}

impl<'a> Pen<'a> {
    pub fn new(w: f64, h: f64, cfg: &'a Config, pal: &'a Palette, scale: i32, icons: &'a mut Icons) -> Option<(ImageSurface, Self)> {
        let s = scale as f64;
        let surf = ImageSurface::create(Format::ARgb32, (w * s) as i32, (h * s) as i32).ok()?;
        surf.set_device_scale(s, s);
        let cr = Context::new(&surf).ok()?;
        let layout = pangocairo::functions::create_layout(&cr);
        Some((surf, Pen { cr, layout, cfg, pal, scale, icons, regions: Vec::new() }))
    }

    pub fn clear(&mut self) {
        self.cr.set_operator(cairo::Operator::Clear);
        let _ = self.cr.paint();
        self.cr.set_operator(cairo::Operator::Over);
        self.regions.clear();
    }

    pub fn set(&self, (r, g, b): Rgb, a: f64) {
        self.cr.set_source_rgba(r, g, b, a);
    }

    fn prepare(&self, s: &str, family: &str, size: f64, weight: Weight, max_w: f64) {
        let mut d = FontDescription::new();
        d.set_family(family);
        d.set_weight(weight);
        d.set_absolute_size(size * pango::SCALE as f64);
        self.layout.set_font_description(Some(&d));
        // Tabular figures keep clocks and percentages from shimmering as they tick.
        let attrs = AttrList::new();
        attrs.insert(AttrFontFeatures::new("tnum"));
        self.layout.set_attributes(Some(&attrs));
        self.layout.set_width((max_w * pango::SCALE as f64) as i32);
        self.layout.set_ellipsize(if max_w > 0.0 { EllipsizeMode::End } else { EllipsizeMode::None });
        self.layout.set_text(s);
    }

    pub fn width(&self, s: &str, size: f64, weight: Weight) -> f64 {
        self.prepare(s, &self.cfg.font, size, weight, -1.0);
        self.layout.pixel_size().0 as f64
    }

    /// Left-aligned at x, vertically centred on cy. Returns the width drawn.
    pub fn text(&self, s: &str, size: f64, weight: Weight, c: Rgb, x: f64, cy: f64, max_w: f64) -> f64 {
        self.prepare(s, &self.cfg.font, size, weight, max_w);
        let (w, h) = self.layout.pixel_size();
        self.set(c, 1.0);
        self.cr.move_to(x, cy - h as f64 / 2.0);
        pangocairo::functions::show_layout(&self.cr, &self.layout);
        w as f64
    }

    /// Wrapped text from a top edge, cut off after `lines`. Returns the height used.
    pub fn wrapped(&self, s: &str, size: f64, c: Rgb, x: f64, y: f64, w: f64, lines: i32) -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        self.prepare(s, &self.cfg.font, size, Weight::Normal, w);
        self.layout.set_wrap(pango::WrapMode::WordChar);
        self.layout.set_height(-lines);
        let h = self.layout.pixel_size().1 as f64;
        self.set(c, 1.0);
        self.cr.move_to(x, y);
        pangocairo::functions::show_layout(&self.cr, &self.layout);
        self.layout.set_height(-1);
        self.layout.set_wrap(pango::WrapMode::Word);
        h
    }

    pub fn right(&self, s: &str, size: f64, weight: Weight, c: Rgb, x_end: f64, cy: f64) -> f64 {
        let w = self.width(s, size, weight);
        self.text(s, size, weight, c, x_end - w, cy, -1.0);
        w
    }

    pub fn pill(&self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let cr = &self.cr;
        let pi = std::f64::consts::PI;
        let r = r.min(w / 2.0).min(h / 2.0);
        cr.new_sub_path();
        cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
        cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
        cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
        cr.arc(x + r, y + r, r, pi, 1.5 * pi);
        cr.close_path();
    }

    pub fn fill(&self, rect: Rect, r: f64, c: Rgb, a: f64) {
        self.pill(rect.0, rect.1, rect.2, rect.3, r);
        self.set(c, a);
        let _ = self.cr.fill();
    }

    /// `size` is the logical edge: rasters can outlive a scale change, so their
    /// density comes from the raster itself rather than from this output.
    pub fn image(&self, img: &ImageSurface, x: f64, y: f64, size: f64, a: f64) {
        let s = img.width() as f64 / size;
        img.set_device_scale(s, s);
        if self.cr.set_source_surface(img, x, y).is_ok() {
            let _ = self.cr.paint_with_alpha(a);
        }
    }

    pub fn symbol(&mut self, name: &'static str, size: f64, x: f64, y: f64, c: Rgb) {
        let Some(mask) = self.icons.symbol(name, (size * self.scale as f64) as i32) else { return };
        mask.set_device_scale(self.scale as f64, self.scale as f64);
        self.set(c, 1.0);
        let _ = self.cr.mask_surface(&mask, x, y);
    }

    /// One-color glyph from the icon font, centred in a `size` box.
    pub fn glyph(&self, g: &str, size: f64, x: f64, y: f64, c: Rgb, a: f64) {
        self.prepare(g, &self.cfg.icon_font, size, Weight::Bold, -1.0);
        let (w, h) = self.layout.pixel_size();
        self.set(c, a);
        self.cr.move_to(x + (size - w as f64) / 2.0, y + (size - h as f64) / 2.0);
        pangocairo::functions::show_layout(&self.cr, &self.layout);
    }

    /// A window's icon. `c` is the glyph's tone; `Color` style ignores it and
    /// shows the app's own icon, or a lettered tile when nothing on disk matches.
    pub fn app(&mut self, appid: &str, size: f64, x: f64, y: f64, c: Rgb, alpha: f64) {
        if self.cfg.icon_style == IconStyle::Glyph {
            return self.glyph(glyph_for(self.cfg, appid), size, x, y, c, alpha);
        }
        if let Some(img) = self.icons.app(appid, (size * self.scale as f64) as i32) {
            return self.image(&img, x, y, size, alpha);
        }
        self.letter(appid, size, x, y, alpha);
    }

    /// Fallback for tray items with no usable icon.
    pub fn letter(&mut self, name: &str, size: f64, x: f64, y: f64, alpha: f64) {
        let tile = mix(self.pal.accent, self.pal.bg, 0.68);
        self.fill((x, y, size, size), size * 0.28, tile, alpha);
        let letter: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
        let w = self.width(&letter, size * 0.56, Weight::Semibold);
        self.text(&letter, size * 0.56, Weight::Semibold, self.pal.fg, x + (size - w) / 2.0, y + size / 2.0, -1.0);
    }

    pub fn button(&mut self, symbol: &'static str, rect: Rect, on: bool, region: Region) {
        if on {
            self.fill(rect, rect.2 / 2.0, self.pal.accent, 0.22);
        }
        let size = 16.0;
        let c = if on { self.pal.accent } else { self.pal.fg };
        self.symbol(symbol, size, rect.0 + (rect.2 - size) / 2.0, rect.1 + (rect.3 - size) / 2.0, c);
        self.regions.push(region);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_lookup_prefers_config_then_exact_then_tail_then_generic() {
        let mut cfg = Config::default();
        assert_eq!(glyph_for(&cfg, "kitty"), glyph_for(&cfg, "org.example.Kitty"));
        assert_eq!(glyph_for(&cfg, "steam_app_42"), GAME_GLYPH);
        assert_eq!(glyph_for(&cfg, "no.such.app"), GENERIC_GLYPH);
        cfg.icons.insert("kitty".into(), "K".into());
        assert_eq!(glyph_for(&cfg, "kitty"), "K");
    }
}
