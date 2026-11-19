//! Drawing of the facility screens: the rows, the notices and the priest's line as quads in the
//! message window's geometry.

use super::GREY;
use super::MARGIN;
use super::MENU;
use super::Notice;
use super::ROW;
use super::SLOTS;
use super::Screen;
use super::Services;
use super::ShopMode;
use super::Source;
use super::VISIBLE_ROWS;
use super::WHITE;
use super::WINDOW;
use super::replace_tag;
use super::service_name;
use super::substitute_values;
use dq9_core::facility::inn;
use dq9_core::party::HELD_ITEMS;
use dq9_core::party::Party;
use dq9_core::talk::Names;
use dq9_formats::Font;
use dq9_render::ui::Atlas;
use dq9_render::ui::Quad;

/// The lines of a message as glyphs, through the game's own layout (`talk::prepare`).
fn glyph_lines(font: &Font<'_>, text: &[u8], width: i32) -> Vec<Vec<(u8, i32)>> {
    let script = dq9_core::talk::prepare(font, text, &Names::default(), width);
    let mut lines = vec![Vec::new()];
    for op in script.ops {
        match op {
            dq9_core::talk::Op::Glyph { glyph, x } => {
                lines.last_mut().unwrap().push((glyph, x));
            }
            dq9_core::talk::Op::Line | dq9_core::talk::Op::Page => lines.push(Vec::new()),
            dq9_core::talk::Op::Name(plate) => {
                lines.last_mut().unwrap().extend(plate.glyphs);
            }
            _ => {}
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

/// Draws a titled window with its rows, the cursor's row highlighted, and a footer.
fn draw_rows(
    atlas: &Atlas,
    font: &Font<'_>,
    title: &[u8],
    rows: &[(Vec<u8>, bool)],
    cursor: Option<usize>,
    footer: &[u8],
) -> Vec<Quad> {
    let mut out = Vec::new();
    atlas.window(WINDOW, crate::talk::FILL, &mut out);
    atlas_text(
        atlas,
        font,
        title,
        WINDOW[0] + MARGIN[0],
        WINDOW[1] + MARGIN[1],
        WHITE,
        &mut out,
    );
    let start = cursor.map_or(0, |cursor| cursor.saturating_sub(3));
    for (visible, (label, enabled)) in rows.iter().skip(start).take(VISIBLE_ROWS).enumerate() {
        let index = start + visible;
        let y = WINDOW[1] + MARGIN[1] + 20 + visible as i32 * ROW;
        if cursor == Some(index) {
            out.push(atlas.solid(
                [WINDOW[0] + 6, y - 2, WINDOW[2] - 12, ROW - 2],
                crate::talk::SELECTED,
            ));
        }
        let color = if *enabled { WHITE } else { GREY };
        let fitted = fit(font, label, WINDOW[2] - 2 * MARGIN[0]);
        atlas_text(
            atlas,
            font,
            &fitted,
            WINDOW[0] + MARGIN[0],
            y,
            color,
            &mut out,
        );
    }
    let fitted = fit(font, footer, WINDOW[2] - 2 * MARGIN[0]);
    atlas_text(
        atlas,
        font,
        &fitted,
        WINDOW[0] + MARGIN[0],
        WINDOW[1] + WINDOW[3] - MARGIN[1] - 8,
        GREY,
        &mut out,
    );
    out
}

fn atlas_text(
    atlas: &Atlas,
    font: &Font<'_>,
    bytes: &[u8],
    x: i32,
    y: i32,
    color: [f32; 4],
    out: &mut Vec<Quad>,
) {
    let (glyphs, _) = font.layout(bytes);
    atlas.text(&glyphs, x, y, color, out);
}

/// Cuts `bytes` to what `width` pixels hold (a whole glyph at a time).
pub(super) fn fit(font: &Font<'_>, bytes: &[u8], width: i32) -> Vec<u8> {
    if font.measure(bytes) <= width {
        return bytes.to_vec();
    }
    let mut out = Vec::new();
    for &byte in bytes {
        out.push(byte);
        if font.measure(&out) > width {
            out.pop();
            break;
        }
    }
    out
}

impl Services {
    /// The quads of the menu on the 256×192 screen; `None` when nothing is open.
    pub fn quads(&self, atlas: &Atlas, font: &Font<'_>, party: &Party) -> Option<Vec<Quad>> {
        if !self.is_open() {
            return None;
        }
        if self.screen == Screen::Notice {
            return Some(self.notice_quads(atlas, font));
        }
        if self.screen == Screen::Priest {
            return Some(self.priest_quads(atlas, font));
        }
        let mut title = Vec::new();
        let mut rows: Vec<(Vec<u8>, bool)> = Vec::new();
        let mut footer = Vec::new();
        let mut cursor = None;
        match self.screen {
            Screen::Closed => return None,
            Screen::Menu => {
                title.extend_from_slice(b"Field");
                for (label, _) in MENU {
                    rows.push((label.to_vec(), true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: close");
            }
            Screen::Items => {
                title.extend_from_slice(
                    format!("Items  bag {}  gold {}", self.bag_rows(party), party.gold).as_bytes(),
                );
                let sources = self.item_sources(party);
                let bag_rows = self.bag_rows(party);
                for (index, source) in sources.iter().enumerate() {
                    let label = match *source {
                        Source::Bag(item) => {
                            let stack = party
                                .bag
                                .slots()
                                .iter()
                                .find(|stack| stack.item == Some(item));
                            let quantity = stack.map_or(0, |stack| stack.quantity);
                            let mut label = self.item_name(item);
                            label.extend_from_slice(format!("  x{quantity}").as_bytes());
                            label
                        }
                        Source::Held(slot) => {
                            let item = party
                                .members
                                .first()
                                .and_then(|member| member.held_items[slot])
                                .unwrap_or(0);
                            let mut label = self.item_name(item);
                            label.extend_from_slice(format!("  (held {})", slot + 1).as_bytes());
                            label
                        }
                    };
                    let _ = index;
                    let _ = bag_rows;
                    rows.push((label, true));
                }
                if rows.is_empty() {
                    rows.push((b"(empty)".to_vec(), false));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: use  H: hold  T: take  X: back");
            }
            Screen::HeldSlots => {
                let item = self.held_item.unwrap_or(0);
                title.extend_from_slice(b"Hold ");
                title.extend_from_slice(&self.item_name(item));
                title.extend_from_slice(b" in...");
                for slot in 0..HELD_ITEMS {
                    let held = party
                        .members
                        .first()
                        .and_then(|member| member.held_items[slot]);
                    let mut label = format!("Slot {}  ", slot + 1).into_bytes();
                    match held {
                        Some(item) => {
                            label.extend_from_slice(&self.item_name(item));
                            rows.push((label, false));
                        }
                        None => {
                            label.extend_from_slice(b"empty");
                            rows.push((label, true));
                        }
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: hold here   X: back");
            }
            Screen::Equip => {
                title.extend_from_slice(b"Equipment");
                let equipment = party.members.first().map(|member| member.equipment);
                for (slot, label) in SLOTS {
                    let item = equipment.and_then(|equipment| equipment.get(slot));
                    let mut row = label.to_vec();
                    row.extend_from_slice(b": ");
                    match item {
                        Some(item) => row.extend_from_slice(&self.item_name(item)),
                        None => row.extend_from_slice(b"(none)"),
                    }
                    rows.push((row, true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::EquipChoice(slot) => {
                title.extend_from_slice(b"Choose ");
                title.extend_from_slice(
                    SLOTS
                        .iter()
                        .find(|(candidate, _)| *candidate == slot)
                        .map_or(b"?", |(_, label)| *label),
                );
                for choice in self.equip_choices(party, slot) {
                    match choice {
                        Some(item) => {
                            let name = self.item_name(item);
                            let stack = party
                                .bag
                                .slots()
                                .iter()
                                .find(|stack| stack.item == Some(item));
                            let mut label = name;
                            if let Some(stack) = stack {
                                label
                                    .extend_from_slice(format!("  x{}", stack.quantity).as_bytes());
                            }
                            rows.push((label, true));
                        }
                        None => rows.push((b"Take it off".to_vec(), true)),
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: equip   X: back");
            }
            Screen::Stats => {
                let index = self.cursor.min(party.members.len().saturating_sub(1));
                title.extend_from_slice(b"Attributes");
                if let Some(member) = party.members.get(index) {
                    let mut line = self.name(index);
                    line.extend_from_slice(
                        format!(
                            "  Lv{}  HP {}/{}  MP {}/{}",
                            member.level(),
                            member.hp,
                            member.max_hp,
                            member.mp,
                            member.max_mp
                        )
                        .as_bytes(),
                    );
                    rows.push((line, true));
                    if let Some(data) = &self.data {
                        let fighter = member.fighter(&data.items, &data.params);
                        rows.push((
                            format!("ATK {}  DEF {}", fighter.attack, fighter.defence).into_bytes(),
                            false,
                        ));
                        rows.push((
                            format!("AGI {}  DFT {}", fighter.agility, fighter.deftness)
                                .into_bytes(),
                            false,
                        ));
                        rows.push((
                            format!(
                                "MGT {}  MDG {}",
                                fighter.magical_might, member.stats.magical_mending
                            )
                            .into_bytes(),
                            false,
                        ));
                    }
                }
                footer.extend_from_slice(b"Up/Down: member   X: back");
            }
            Screen::Spells => {
                title.extend_from_slice(b"Healing spells");
                let spells = self.spells(party);
                if spells.is_empty() {
                    rows.push((b"(none learned)".to_vec(), false));
                }
                for (action, cost, usable) in spells {
                    let mut label = self
                        .data
                        .as_ref()
                        .and_then(|data| data.lines.get(&action))
                        .map(|lines| lines.name.clone())
                        .unwrap_or_else(|| format!("#{action}").into_bytes());
                    label.extend_from_slice(format!("  MP {cost}").as_bytes());
                    rows.push((label, usable));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: cast   X: back");
            }
            Screen::Targets => {
                title.extend_from_slice(b"On whom?");
                for (index, member) in party.members.iter().enumerate() {
                    let mut label = self.name(index);
                    label.extend_from_slice(
                        format!("  HP {}/{}", member.hp, member.max_hp).as_bytes(),
                    );
                    rows.push((label, member.hp > 0));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::Shop => {
                title.extend_from_slice(
                    format!(
                        "Shop {}  {}  gold {}",
                        self.shop_number,
                        match self.shop_mode {
                            ShopMode::Buy => "Buy",
                            ShopMode::Sell => "Sell",
                        },
                        party.gold
                    )
                    .as_bytes(),
                );
                for (label, _) in self.shop_rows(party) {
                    rows.push((label, true));
                }
                if rows.is_empty() {
                    rows.push((b"(empty)".to_vec(), false));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: trade   Left/Right: buy/sell   X: leave");
            }
            Screen::Inn => {
                let price = self
                    .inn
                    .as_ref()
                    .map_or(0, |inn| inn::price(inn.per_head, party));
                title.extend_from_slice(
                    format!("Inn  {} gold  {} beds", price, inn::living(party)).as_bytes(),
                );
                if let Some(inn) = &self.inn {
                    for choice in &inn.labels {
                        rows.push((choice.clone(), true));
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: leave");
            }
            Screen::Church => {
                title.extend_from_slice(b"Church");
                if let Some(church) = &self.church {
                    for service in &church.services {
                        let label = church
                            .messages
                            .get(&service.label())
                            .cloned()
                            .unwrap_or_else(|| format!("{service:?}").into_bytes());
                        rows.push((label, true));
                    }
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: leave");
            }
            Screen::ChurchMembers => {
                let service = self.church_service();
                title.extend_from_slice(
                    format!("{}: on whom?", service.map_or("?", |s| service_name(s))).as_bytes(),
                );
                for (index, member) in party.members.iter().enumerate() {
                    let mut label = self.name(index);
                    if member.hp == 0 {
                        label.extend_from_slice(b"  (dead)");
                    }
                    let status = self.statuses.get(index).copied().unwrap_or_default();
                    if status.poisoned {
                        label.extend_from_slice(b"  (poisoned)");
                    }
                    if status.cursed {
                        label.extend_from_slice(b"  (cursed)");
                    }
                    rows.push((label, true));
                }
                cursor = Some(self.cursor);
                footer.extend_from_slice(b"Enter: choose   X: back");
            }
            Screen::ChurchConfirm => {
                let cure = self.church.as_ref().and_then(|church| church.cure);
                if let Some((service, _, price)) = cure {
                    title.extend_from_slice(
                        format!("{}: {price} gold", service_name(service)).as_bytes(),
                    );
                }
                rows.push((b"Yes".to_vec(), true));
                rows.push((b"No".to_vec(), true));
                cursor = Some(self.church.as_ref().map_or(0, |church| church.confirm));
                footer.extend_from_slice(b"Enter: answer   X: back");
            }
            Screen::Notice => {}
            Screen::Priest => {}
        }
        Some(draw_rows(atlas, font, &title, &rows, cursor, &footer))
    }

    /// The bytes a notice draws: `<val_n>`, the target and the vocation label replaced; an
    /// unresolved tag remains visible rather than pretending a name is known.
    pub(super) fn resolved_notice_text(&self, notice: &Notice) -> Vec<u8> {
        let mut text = substitute_values(&notice.text, &notice.values);
        if let Some(target) = &notice.target {
            replace_tag(&mut text, b"<TARGET>", target);
            replace_tag(&mut text, b"<TARGET0>", target);
            replace_tag(&mut text, b"<Target0>", target);
        }
        if let Some(vocation) = &notice.vocation {
            replace_tag(&mut text, b"<STR_2>", vocation);
            replace_tag(&mut text, b"<str_2>", vocation);
        }
        text
    }

    /// A notice: the facility's own line laid out with the font, with the `<val_n>` values
    /// substituted; an application message is drawn the same way.
    fn notice_quads(&self, atlas: &Atlas, font: &Font<'_>) -> Vec<Quad> {
        let mut out = Vec::new();
        atlas.window(WINDOW, crate::talk::FILL, &mut out);
        let Some(notice) = &self.notice else {
            return out;
        };
        let text = self.resolved_notice_text(notice);
        let lines = glyph_lines(font, &text, WINDOW[2] - 2 * MARGIN[0]);
        for (index, line) in lines.iter().take(VISIBLE_ROWS).enumerate() {
            let y = WINDOW[1] + MARGIN[1] + 20 + index as i32 * ROW;
            atlas.text(line, WINDOW[0] + MARGIN[0], y, WHITE, &mut out);
        }
        let footer = b"Enter: continue";
        atlas_text(
            atlas,
            font,
            footer,
            WINDOW[0] + MARGIN[0],
            WINDOW[1] + WINDOW[3] - MARGIN[1] - 8,
            GREY,
            &mut out,
        );
        out
    }

    /// The church in mode 2: the priest's line in the same window as a notice, no title and
    /// no menu; a key ends the visit (`func_ov003_0215af9c`, states 10 and 11).
    fn priest_quads(&self, atlas: &Atlas, font: &Font<'_>) -> Vec<Quad> {
        let mut out = Vec::new();
        atlas.window(WINDOW, crate::talk::FILL, &mut out);
        let Some(priest) = &self.priest else {
            return out;
        };
        let lines = glyph_lines(font, &priest.text, WINDOW[2] - 2 * MARGIN[0]);
        for (index, line) in lines.iter().take(VISIBLE_ROWS).enumerate() {
            let y = WINDOW[1] + MARGIN[1] + index as i32 * ROW;
            atlas.text(line, WINDOW[0] + MARGIN[0], y, WHITE, &mut out);
        }
        let footer = b"Enter: continue";
        atlas_text(
            atlas,
            font,
            footer,
            WINDOW[0] + MARGIN[0],
            WINDOW[1] + WINDOW[3] - MARGIN[1] - 8,
            GREY,
            &mut out,
        );
        out
    }
}
