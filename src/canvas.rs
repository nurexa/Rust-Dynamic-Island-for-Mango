//! One layer surface's shared-memory buffers. The island, the sidebar and the edge
//! strips all draw through this: it finds a free buffer, clears only what that
//! buffer held, and damages only what changed on screen.
use crate::pen::Rect;
use cairo::{Context, Format, ImageSurface};
use smithay_client_toolkit::{
    reexports::client::protocol::{wl_shm, wl_surface::WlSurface},
    shm::{Shm, slot::{Buffer, SlotPool}},
};

pub struct Canvas {
    pool: SlotPool,
    /// Each buffer with the bounds it last held: outside them it's already clear.
    buffers: Vec<(Buffer, Rect)>,
    /// Which full drawing each buffer last got, and the newest one's number.
    drawn: Vec<u64>,
    newest: u64,
    size: (f64, f64),
    scale: i32,
    /// What's on screen now.
    shown: Rect,
}

/// A buffer being drawn. `Canvas::present` puts it on screen.
pub struct Frame {
    pub cr: Context,
    target: ImageSurface,
    index: usize,
    bounds: Rect,
    /// All that changed on screen, when it's less than the bounds.
    changed: Option<Rect>,
    /// The only part of this buffer being redrawn, when it isn't the whole of it.
    pub patch: Option<Rect>,
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.0.min(b.0), a.1.min(b.1));
    (x, y, (a.0 + a.2).max(b.0 + b.2) - x, (a.1 + a.3).max(b.1 + b.3) - y)
}

impl Canvas {
    pub fn new(shm: &Shm, size: (f64, f64), scale: i32) -> Option<Self> {
        let bytes = (size.0 * size.1).max(1.0) as usize * 4 * scale.pow(2) as usize;
        let pool = SlotPool::new(bytes, shm).ok()?;
        Some(Canvas { pool, buffers: Vec::new(), drawn: Vec::new(), newest: 0, size, scale, shown: (0.0, 0.0, size.0, size.1) })
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }

    pub fn resize(&mut self, size: (f64, f64), scale: i32) {
        if (size, scale) != (self.size, self.scale) {
            (self.size, self.scale) = (size, scale);
            self.buffers.clear();
            self.drawn.clear();
            self.shown = (0.0, 0.0, size.0, size.1);
        }
    }

    /// A cleared buffer to draw `bounds` in; nothing outside them survives.
    pub fn begin(&mut self, bounds: Rect) -> Option<Frame> {
        self.newest += 1;
        self.draw(bounds, None)
    }

    /// The same picture as the last `begin`, but for what's in `changed`. A buffer
    /// that has that picture repaints only there; one that's behind catches up whole.
    pub fn begin_live(&mut self, bounds: Rect, changed: Rect) -> Option<Frame> {
        self.draw(bounds, Some(changed))
    }

    fn draw(&mut self, bounds: Rect, changed: Option<Rect>) -> Option<Frame> {
        let s = self.scale as f64;
        let (bw, bh) = ((self.size.0 * s) as i32, (self.size.1 * s) as i32);
        let stride = bw * 4;
        let free = self.buffers.iter().position(|(b, _)| b.canvas(&mut self.pool).is_some());
        let index = match free {
            Some(i) => i,
            None => {
                let (buffer, _) = self.pool.create_buffer(bw, bh, stride, wl_shm::Format::Argb8888).ok()?;
                // A recycled slot can hold anything.
                self.buffers.push((buffer, (0.0, 0.0, self.size.0, self.size.1)));
                self.drawn.push(0);
                self.buffers.len() - 1
            }
        };
        let stale = std::mem::replace(&mut self.buffers[index].1, bounds);
        let data = self.buffers[index].0.canvas(&mut self.pool)?;
        let (ptr, len) = (data.as_mut_ptr(), data.len());
        debug_assert!(len >= (stride * bh) as usize);
        // Safety: `data` is an exclusive view of the shm slot, valid until the buffer
        // is attached in `present`, and the cairo surface is finished before that.
        let target = unsafe { ImageSurface::create_for_data_unsafe(ptr, Format::ARgb32, bw, bh, stride) }.ok()?;
        target.set_device_scale(s, s);
        let cr = Context::new(&target).ok()?;
        let current = std::mem::replace(&mut self.drawn[index], self.newest) == self.newest;
        let only = changed.filter(|_| current);
        let (x, y, w, h) = only.unwrap_or_else(|| union(bounds, stale));
        let (x, y, w, h) = (x.floor(), y.floor(), (x + w).ceil() - x.floor(), (y + h).ceil() - y.floor());
        if only.is_some() {
            cr.rectangle(x, y, w, h);
            cr.clip();
        }
        cr.rectangle(x, y, w, h);
        cr.set_operator(cairo::Operator::Clear);
        let _ = cr.fill();
        cr.set_operator(cairo::Operator::Over);
        Some(Frame { cr, target, index, bounds, changed, patch: only.map(|_| (x, y, w, h)) })
    }

    /// Attach and damage. The caller commits, along with whatever else changed.
    pub fn present(&mut self, frame: Frame, surface: &WlSurface) {
        let Frame { cr, target, index, bounds, changed, .. } = frame;
        drop(cr);
        target.finish();
        let (x, y, w, h) = changed.unwrap_or_else(|| union(bounds, self.shown));
        self.shown = bounds;
        let s = self.scale as f64;
        surface.set_buffer_scale(self.scale);
        let _ = self.buffers[index].0.attach_to(surface);
        surface.damage_buffer((x * s).floor() as i32 - 2, (y * s).floor() as i32 - 2,
                              (w * s).ceil() as i32 + 4, (h * s).ceil() as i32 + 4);
    }
}
