//! The drawing kit modules share, and the types they hand back. Both island
//! states render once into a cached surface with their click targets, and the
//! frame loop just composites those. Nothing here runs per animation frame.
use crate::{
    bus::MenuEntry,
    icons::{self, Icons},
    settings::{Config, IconStyle},
    theme::{Palette, Rgb, mix},
    viz::Live,
};
use cairo::{Context, Format, ImageSurface};
use pango::{AttrFontFeatures, AttrList, EllipsizeMode, FontDescription, Weight};
use std::{cell::RefCell, collections::HashMap};

/// Text, icon font, size bits, weight, max width, scale.
type ShapeKey = (String, bool, u64, Weight, i64, u64);

thread_local! {
    static SHAPED: RefCell<HashMap<ShapeKey, pango::Layout>> = RefCell::new(HashMap::new());
}

pub type Rect = (f64, f64, f64, f64);

/// Shaped text is kept by what it says, not the font it was shaped in.
pub fn fonts_changed() {
    SHAPED.with_borrow_mut(|cache| cache.clear());
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    View { tag: u32, mon: String },
    MoveTo(u32),
    Focus(u64),
    Sh(String),
    Mute,
    Volume,
    Brightness,
    Night,
    Warmth,
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
    /// Open the inline reply field.
    NoteReply(u32),
    /// Expand or collapse one app's history.
    NoteGroup(String),
    /// Dismiss everything from one app.
    NoteClearApp(String),
    Dnd,
    /// One of the island's own commands, as `island <cmd>` would send it.
    Cmd(String),
    /// A monitor's brightness over DDC, by I2C bus.
    Ddc(u32),
    /// An app's own volume, by its stream index.
    StreamVolume(u32),
    StreamMute(u32),
    /// Make this output (or with `source`, this microphone) the default.
    AudioDevice { source: bool, name: String },
    /// Turns the Bluetooth adapter on or off.
    Bluetooth,
    Sidebar,
    /// Page the calendar by months; 0 comes back to this one.
    CalMonth(i32),
    CalDay(i64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    Tags,
    Volume,
    Palette,
    /// A panel with more than fits.
    Panel,
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
    /// Where the split style's bubble starts.
    pub split: Option<f64>,
    /// Where visualizers get painted over it.
    pub live: Vec<Live>,
}

impl Content {
    /// What's under a point in its own coordinates. Past its edges is out of reach.
    pub fn hit(&self, (x, y): (f64, f64)) -> Option<usize> {
        if y < 0.0 || y >= self.h {
            return None;
        }
        // Icons are pushed after their chip, so search top-most first.
        self.regions.iter().rposition(|r| x >= r.rect.0 && x < r.rect.0 + r.rect.2 && y >= r.rect.1 && y < r.rect.1 + r.rect.3)
    }

    /// The region still doing what `pressed` did. Content can change under a press, so
    /// an index proves nothing: the one at the same index may be another notification.
    pub fn same(&self, at: Option<usize>, pressed: &Region) -> Option<&Region> {
        self.regions.get(at?).filter(|r| r.left == pressed.left && r.right == pressed.right && r.middle == pressed.middle)
    }
}

impl Region {
    /// Nothing happens on a click, so it mustn't look as if something would.
    pub fn inert(&self) -> bool {
        self.left == Action::None && self.right == Action::None && self.middle == Action::None
    }
}

/// Content scrolled up by `by`: only what's in `room` can be pointed at or painted live.
pub fn scrolled(regions: &mut Vec<Region>, live: &mut Vec<Live>, by: f64, room: f64) {
    regions.retain_mut(|r| {
        let (top, bottom) = ((r.rect.1 - by).max(0.0), (r.rect.1 - by + r.rect.3).min(room));
        (r.rect.1, r.rect.3) = (top, bottom - top);
        bottom > top
    });
    live.retain_mut(|l| {
        l.rect.1 -= by;
        l.rect.1 >= 0.0 && l.rect.1 + l.rect.3 <= room
    });
}

/// Regions that follow the pointer while it's held.
pub fn is_slider(a: &Action) -> bool {
    matches!(a, Action::Volume | Action::Brightness | Action::Warmth | Action::StreamVolume(_) | Action::Ddc(_))
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
    ("alacritty", "terminal"),
    ("blender", "󰂫"),
    ("brave-browser", "󰊯"),
    ("chromium", "󰊯"),
    ("code", "󰨞"),
    ("code-oss", "󰨞"),
    ("com.obsproject.studio", "󰑋"),
    ("discord", "󰙯"),
    ("dolphin", "folder"),
    ("firefox", "󰈹"),
    ("foot", "terminal"),
    ("ghostty", "terminal"),
    ("google-chrome", "󰊯"),
    ("kitty", "terminal"),
    ("konsole", "terminal"),
    ("librewolf", "󰈹"),
    ("mpv", "󰐹"),
    ("nautilus", "folder"),
    ("nyachan", "sports_esports"),
    ("obsidian", "description"),
    ("org.kde.dolphin", "folder"),
    ("org.prismlauncher.prismlauncher", "󰍳"),
    ("org.vinegarhq.sober", "sports_esports"),
    ("org.vinegarhq.vinegar", "sports_esports"),
    ("pavucontrol", "volume_up"),
    ("resolve", "󰕧"),
    ("spotify", "󰓇"),
    ("steam", "󰓓"),
    ("thorium-browser", "󰊯"),
    ("thunar", "folder"),
    ("vesktop", "󰙯"),
    ("vlc", "󰕼"),
    ("wezterm", "terminal"),
    ("wootility", "keyboard"),
    ("zen", "󰈹"),
    ("zen-browser", "󰈹"),
];
// The island's own rounded icons for kinds of app; the font only for brands' marks.
const GENERIC_GLYPH: &str = "web_asset";
const GAME_GLYPH: &str = "sports_esports";

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
    /// Pixels per unit.
    pub scale: f64,
    /// The width modules lay out to.
    pub w: f64,
    pub live: Vec<Live>,
    pub icons: &'a mut Icons,
    pub regions: Vec<Region>,
    /// How solid the steps above and below the island are: less where it's see-through.
    pub solid: f64,
}

impl<'a> Pen<'a> {
    pub fn new(w: f64, h: f64, cfg: &'a Config, pal: &'a Palette, scale: crate::scale::Scale, icons: &'a mut Icons) -> Option<(ImageSurface, Self)> {
        let s = scale.px();
        let surf = ImageSurface::create(Format::ARgb32, (w * s).ceil() as i32, (h * s).ceil() as i32).ok()?;
        surf.set_device_scale(s, s);
        let cr = Context::new(&surf).ok()?;
        let layout = pangocairo::functions::create_layout(&cr);
        Some((surf, Pen { cr, layout, cfg, pal, scale: s, w, live: Vec::new(), icons, regions: Vec::new(), solid: 1.0 }))
    }

    pub fn clear(&mut self) {
        self.cr.set_operator(cairo::Operator::Clear);
        let _ = self.cr.paint();
        self.cr.set_operator(cairo::Operator::Over);
        self.regions.clear();
        self.live.clear();
    }

    pub fn set(&self, (r, g, b): Rgb, a: f64) {
        self.cr.set_source_rgba(r, g, b, a);
    }

    fn prepare(&self, s: &str, family: &str, size: f64, weight: Weight, max_w: f64) {
        configure(&self.layout, s, family, size, weight, max_w);
    }

    /// One line shaped once and kept: a pill rebuild draws the same few strings
    /// every time, and shaping them was most of what it cost.
    fn shaped(&self, s: &str, icon: bool, size: f64, weight: Weight, max_w: f64) -> pango::Layout {
        let key = (s.to_owned(), icon, size.to_bits(), weight, max_w as i64, self.scale.to_bits());
        SHAPED.with_borrow_mut(|cache| {
            if let Some(l) = cache.get(&key) {
                return l.clone();
            }
            // ponytail: dropped wholesale when full; an LRU if churn ever shows up
            if cache.len() >= 96 {
                cache.clear();
            }
            let l = pango::Layout::new(&self.layout.context());
            configure(&l, s, if icon { &self.cfg.icon_font } else { &self.cfg.font }, size, weight, max_w);
            cache.insert(key, l.clone());
            l
        })
    }

    pub fn width(&self, s: &str, size: f64, weight: Weight) -> f64 {
        self.shaped(s, false, size, weight, -1.0).pixel_size().0 as f64
    }

    /// Left-aligned at x, vertically centred on cy. Returns the width drawn.
    pub fn text(&self, s: &str, size: f64, weight: Weight, c: Rgb, x: f64, cy: f64, max_w: f64) -> f64 {
        let layout = self.shaped(s, false, size, weight, max_w);
        let (w, h) = layout.pixel_size();
        self.set(c, 1.0);
        self.cr.move_to(x, cy - h as f64 / 2.0);
        pangocairo::functions::show_layout(&self.cr, &layout);
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
        rounded_rect(&self.cr, x, y, w, h, r);
    }

    pub fn fill(&self, rect: Rect, r: f64, c: Rgb, a: f64) {
        self.pill(rect.0, rect.1, rect.2, rect.3, r);
        self.set(c, a);
        let _ = self.cr.fill();
    }

    /// Depth is three flat steps: the island, a tile on it, a control on a tile. A tile
    /// is its own face with a faint rim on top; no drops or gradients, which stack
    /// into mud through glass and on saturated palettes.
    pub fn surface(&self, rect: Rect, r: f64, face: Rgb) {
        let (x, y, w, h) = rect;
        self.fill(rect, r, face, self.solid);
        let rim = cairo::LinearGradient::new(0.0, y, 0.0, y + h.min(2.0 * r));
        let (er, eg, eb) = self.pal.edge;
        rim.add_color_stop_rgba(0.0, er, eg, eb, if self.pal.dark { 0.16 } else { 0.5 });
        rim.add_color_stop_rgba(1.0, er, eg, eb, 0.0);
        self.pill(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r - 0.5);
        self.cr.set_line_width(1.0);
        if self.cr.set_source(&rim).is_ok() {
            let _ = self.cr.stroke();
        }
    }

    /// What can be pressed: a veil of ink over whatever it sits on, so it's the same
    /// step on the island, on a tile, through glass or in the light.
    pub fn raised(&self, rect: Rect, r: f64) {
        self.fill(rect, r, self.pal.fg, if self.pal.dark { crate::theme::VEIL.0 } else { crate::theme::VEIL.1 });
    }

    /// A step down: tracks, fields and whatever holds a set of things.
    pub fn well(&self, rect: Rect, r: f64) {
        self.fill(rect, r, self.pal.fg, if self.pal.dark { crate::theme::VEIL.0 + 0.01 } else { crate::theme::VEIL.1 + 0.01 });
    }

    /// A surface under what's already drawn, for a card that's as tall as what's on it.
    pub fn behind(&self, rect: Rect, r: f64, face: Rgb) {
        let (x, y, w, h) = rect;
        let _ = self.cr.save();
        self.cr.rectangle(x - 3.0, y - 3.0, w + 6.0, h + 7.0);
        self.cr.clip();
        self.cr.push_group();
        self.surface(rect, r, face);
        if self.cr.pop_group_to_source().is_ok() {
            self.cr.set_operator(cairo::Operator::DestOver);
            let _ = self.cr.paint();
        }
        let _ = self.cr.restore();
    }

    /// A slider: a track cut in, what's full of it, and a knob that stands out.
    pub fn slider(&self, x: f64, cy: f64, w: f64, frac: f64, tone: Rgb) {
        let at = w * frac.clamp(0.0, 1.0);
        self.well((x, cy - 4.0, w, 8.0), 4.0);
        if at > 0.0 {
            self.fill((x, cy - 4.0, at.max(8.0), 8.0), 4.0, tone, 1.0);
        }
        let knob = x + at.clamp(0.0, w);
        self.fill((knob - 7.5, cy - 6.0, 15.0, 15.0), 7.5, (0.0, 0.0, 0.0), 0.22);
        self.fill((knob - 7.0, cy - 7.0, 14.0, 14.0), 7.0, if self.pal.dark { self.pal.fg } else { tone }, 1.0);
    }

    /// How far along something is, nothing to drag.
    pub fn meter(&self, (x, y, w, h): Rect, frac: f64) {
        self.well((x, y, w, h), h / 2.0);
        if frac > 0.0 {
            self.fill((x, y, (w * frac.min(1.0)).max(h), h), h / 2.0, self.pal.accent, 1.0);
        }
    }

    pub fn switch(&self, x: f64, cy: f64, on: bool) {
        let track = (x, cy - 10.0, 36.0, 20.0);
        let knob = if on {
            self.surface(track, 10.0, self.pal.accent);
            self.pal.on_accent
        } else {
            self.well(track, 10.0);
            self.pal.muted
        };
        self.fill((if on { x + 19.0 } else { x + 3.0 }, cy - 7.0, 14.0, 14.0), 7.0, knob, 1.0);
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
        self.icon(name, false, size, x, y, c, 1.0);
    }

    /// One of the island's own icons: an outline, or solid with `fill` for a state that's on.
    #[allow(clippy::too_many_arguments)]
    pub fn icon(&mut self, name: &'static str, fill: bool, size: f64, x: f64, y: f64, c: Rgb, a: f64) {
        let s = self.scale;
        let solid = fill || self.cfg.ui.solid_icons;
        let Some(mask) = self.icons.symbol(name, solid, (size * s).round() as i32) else { return };
        mask.set_device_scale(s, s);
        self.set(c, a);
        // On whole device pixels, or every stroke goes soft.
        let _ = self.cr.mask_surface(&mask, (x * s).round() / s, (y * s).round() / s);
    }

    /// Whatever a config or a tool names as its icon: one of the island's own, by
    /// name, or else a character from the icon font.
    #[allow(clippy::too_many_arguments)]
    pub fn mark(&mut self, what: &str, fill: bool, size: f64, x: f64, y: f64, c: Rgb, a: f64) {
        match icons::builtin(what) {
            Some(name) => self.icon(name, fill, size, x, y, c, a),
            None => self.glyph(what, size, x, y, c, a),
        }
    }

    /// One-color glyph from the icon font, centred in a `size` box.
    pub fn glyph(&self, g: &str, size: f64, x: f64, y: f64, c: Rgb, a: f64) {
        let layout = self.shaped(g, true, size, Weight::Bold, -1.0);
        let (w, h) = layout.pixel_size();
        self.set(c, a);
        self.cr.move_to(x + (size - w as f64) / 2.0, y + (size - h as f64) / 2.0);
        pangocairo::functions::show_layout(&self.cr, &layout);
    }

    /// A window's icon. `c` is the glyph's tone; `Color` style ignores it and
    /// shows the app's own icon, or a lettered tile when nothing on disk matches.
    pub fn app(&mut self, appid: &str, size: f64, x: f64, y: f64, c: Rgb, alpha: f64) {
        if self.cfg.icon_style == IconStyle::Glyph {
            return self.mark(glyph_for(self.cfg, appid), false, size, x, y, c, alpha);
        }
        if let Some(img) = self.icons.app(appid, (size * self.scale) as i32) {
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
            self.surface(rect, rect.2 / 2.0, self.pal.accent);
        }
        let size = 18.0;
        let c = if on { self.pal.on_accent } else { self.pal.fg };
        self.icon(symbol, on, size, rect.0 + (rect.2 - size) / 2.0, rect.1 + (rect.3 - size) / 2.0, c, 1.0);
        self.regions.push(region);
    }
}

pub fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let pi = std::f64::consts::PI;
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

fn configure(l: &pango::Layout, s: &str, family: &str, size: f64, weight: Weight, max_w: f64) {
    let mut d = FontDescription::new();
    d.set_family(family);
    d.set_weight(weight);
    d.set_absolute_size(size * pango::SCALE as f64);
    l.set_font_description(Some(&d));
    // Tabular figures keep clocks and percentages from shimmering as they tick.
    let attrs = AttrList::new();
    attrs.insert(AttrFontFeatures::new("tnum"));
    l.set_attributes(Some(&attrs));
    l.set_width((max_w * pango::SCALE as f64) as i32);
    l.set_ellipsize(if max_w > 0.0 { EllipsizeMode::End } else { EllipsizeMode::None });
    l.set_text(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolled_content_takes_the_click_where_its_drawn() {
        let row = |id: u32, y: f64| Region::new((0.0, y, 100.0, 40.0), 10.0, Action::NoteDismiss(id));
        let mut regions = vec![row(1, 0.0), row(2, 40.0), row(3, 80.0), row(4, 120.0)];
        scrolled(&mut regions, &mut Vec::new(), 60.0, 90.0);
        let surf = ImageSurface::create(Format::ARgb32, 1, 1).unwrap();
        let c = Content { surf, w: 100.0, h: 90.0, regions, split: None, live: Vec::new() };
        let on = |y: f64| c.hit((50.0, y)).map(|i| c.regions[i].left.clone());
        assert_eq!(on(0.0), Some(Action::NoteDismiss(2)));
        assert_eq!(on(19.9), Some(Action::NoteDismiss(2)));
        assert_eq!(on(20.0), Some(Action::NoteDismiss(3)));
        assert_eq!(on(89.9), Some(Action::NoteDismiss(4)));
        assert_eq!(on(90.0), None, "below the room is out of reach");
        assert_eq!(on(-1.0), None);
        assert!(c.regions.iter().all(|r| r.rect.1 >= 0.0 && r.rect.1 + r.rect.3 <= 90.0), "no highlight past the room");
        // The same index, another notification: not what was pressed.
        assert!(c.same(Some(0), &row(1, 0.0)).is_none());
        assert!(c.same(Some(0), &row(2, 0.0)).is_some());
    }

    #[test]
    fn cached_shaping_matches_a_fresh_shape() {
        let (cfg, pal, mut icons) = (Config::default(), crate::theme::load(""), Icons::new());
        let (_s, p) = Pen::new(200.0, 40.0, &cfg, &pal, crate::scale::Scale { buffer: 1, ui: 1.0 }, &mut icons).unwrap();
        let fresh = |s: &str| {
            p.prepare(s, &cfg.font, 12.5, Weight::Bold, -1.0);
            p.layout.pixel_size().0 as f64
        };
        for s in ["1", "12:34 PM", "1", "+3"] {
            assert_eq!(p.width(s, 12.5, Weight::Bold), fresh(s), "{s}");
        }
        // Size is part of the key: a hit must not hand back another size's layout.
        assert!(p.width("12:34 PM", 20.0, Weight::Bold) > p.width("12:34 PM", 12.5, Weight::Bold));
    }

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
