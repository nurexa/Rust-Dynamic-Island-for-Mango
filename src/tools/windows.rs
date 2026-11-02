//! Every open window, on any workspace or monitor.
use super::{Act, Ctx, Icon, Item};
use crate::pen::glyph_for;

pub fn list(ctx: &Ctx) -> Vec<Item> {
    ctx.clients.iter().map(|c| {
        let appid = c.appid.as_deref().unwrap_or("");
        let title = c.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| appid.to_owned());
        let tags: Vec<String> = c.tags.iter().map(|t| (t % 10).to_string()).collect();
        Item::new(title, Icon::Glyph(glyph_for(&ctx.cfg, appid).to_owned()), Act::Focus(c.id))
            .sub(format!("{appid} · workspace {}", tags.join(", ")))
    }).collect()
}
