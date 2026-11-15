use super::{Arc, DocInfo, DocView, Edit, HashMap, PageText, Pos2, Range, Rect, comments, pos2};

/// A text selection on one page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Selection {
    pub(super) page: usize,
    pub(super) span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Span {
    /// Reading order, between two glyph indices.
    Run { anchor: usize, head: usize },
    /// Column select (#740): the glyphs inside a rectangle in page view space, drawn from the
    /// corner the drag started at (`rect[0..2]`) to the pointer (`rect[2..4]`).
    Column { rect: [f32; 4] },
}

impl Selection {
    pub(super) fn run(page: usize, anchor: usize, head: usize) -> Self {
        Self { page, span: Span::Run { anchor, head } }
    }

    fn range(anchor: usize, head: usize) -> Range<usize> {
        anchor.min(head)..anchor.max(head).saturating_add(1)
    }

    /// One rectangle per selected piece of a line, in view space (highlighting, markup quads).
    pub(super) fn rects(&self, text: &PageText) -> Vec<[f32; 4]> {
        match self.span {
            Span::Run { anchor, head } => text.line_rects(Self::range(anchor, head)),
            Span::Column { rect } => text.glyph_rects(&text.glyphs_in(rect)),
        }
    }

    /// The selected text, as Copy puts it on the clipboard.
    pub(super) fn text(&self, text: &PageText) -> String {
        match self.span {
            Span::Run { anchor, head } => text.text_of(Self::range(anchor, head)),
            Span::Column { rect } => text.column_text(&text.glyphs_in(rect)),
        }
    }
}

/// The glyphs `clicks` quick clicks select around glyph `a`: two the word, three the line, more
/// the page.
pub(super) fn click_unit(text: &PageText, clicks: u32, a: usize) -> Option<(usize, usize)> {
    match clicks {
        2 => text.word_at(a),
        3 => text.line_at(a),
        _ => text.glyphs.len().checked_sub(1).map(|last| (0, last)),
    }
}

impl DocView {
    /// The text selection as markup quadrilaterals in user space: (page, quads).
    pub(crate) fn selection_quads(&self, info: &DocInfo) -> Option<(usize, Vec<[f64; 8]>)> {
        let s = self.selection?;
        let text = self.texts.get(&s.page)?;
        let page = info.pages.get(s.page)?;
        let quads: Vec<[f64; 8]> = s.rects(text).into_iter().map(|r| page.view_rect_to_quad(r)).collect();
        (!quads.is_empty()).then_some((s.page, quads))
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
        self.actions_dismissed = None;
    }

    /// Select text on `page` from glyph `from` to glyph `to` (tests and automation).
    pub fn select_text(&mut self, page: usize, from: usize, to: usize) {
        self.all_text = false;
        self.selection = Some(Selection::run(page, from, to));
    }

    /// Screen position of the centre of glyph `glyph` on `page`, if that page is on screen and its
    /// text layer is loaded (used by UI tests and automation to aim pointer input).
    pub fn glyph_screen_pos(&self, page: usize, glyph: usize) -> Option<Pos2> {
        self.glyph_screen_rect(page, glyph).map(|r| r.center())
    }

    /// The screen rectangle of glyph `glyph` on `page`, when the page is on screen.
    pub fn glyph_screen_rect(&self, page: usize, glyph: usize) -> Option<Rect> {
        let (_, xf) = self.screen_xforms.iter().find(|(p, _)| *p == page)?;
        let g = self.texts.get(&page)?.glyphs.get(glyph)?;
        Some(xf.view_rect(g.rect))
    }

    /// Selected text, if any (⌘C).
    /// The selected text: of the pages read so far, after Select All of the document (Copy reads
    /// the others, [`DocView::pages_without_text`]).
    pub fn selected_text(&self) -> Option<String> {
        if self.all_text {
            return Some(self.whole_text(&HashMap::new())).filter(|x| !x.is_empty());
        }
        let s = self.selection?;
        let t = self.texts.get(&s.page)?;
        Some(s.text(t)).filter(|x| !x.is_empty())
    }

    /// The selection's page, its text layer and the selected glyphs in copy order, for Copy with
    /// formatting. `None` after Select All of the document and for a column selection, which
    /// copy plain text (a column's rows and tab-separated cells are not paragraphs).
    pub(crate) fn selected_glyphs(&self) -> Option<(usize, Arc<PageText>, Vec<usize>)> {
        if self.all_text {
            return None;
        }
        let s = self.selection?;
        let t = self.texts.get(&s.page)?;
        let glyphs = match s.span {
            Span::Run { anchor, head } => Selection::range(anchor, head).take(t.glyphs.len()).collect(),
            Span::Column { .. } => return None,
        };
        Some((s.page, t.clone(), glyphs))
    }

    /// Select every page without navigating; shared by Organize and the Pages sidebar.
    pub fn select_all_pages(&mut self) -> bool {
        self.selected = (0..self.page_count).collect();
        self.select_anchor = (self.page_count > 0).then_some(self.current);
        !self.selected.is_empty()
    }

    /// Select all words on the current page, or every page in Organize.
    pub fn select_all(&mut self) -> bool {
        if self.organize {
            return self.select_all_pages();
        }
        // Scrolling continuously, Select All takes the whole document's text (#2639); one page at
        // a time, the page shown.
        // Like the page's, true once some text is selected: the pages whose text isn't read yet are
        // read when the selection is copied.
        if self.continuous && self.page_count > 1 {
            self.selection = None;
            self.all_text = true;
            return self.selected_text().is_some();
        }
        self.all_text = false;
        let page = self.current;
        let Some(t) = self.texts.get(&page) else { return false };
        if t.glyphs.is_empty() {
            return false;
        }
        self.selection = Some(Selection::run(page, 0, t.glyphs.len() - 1));
        true
    }

    /// Pages whose text [`DocView::whole_text`] still needs: not read yet, nor failed.
    pub(crate) fn pages_without_text(&self) -> Vec<usize> {
        (0..self.page_count).filter(|p| !self.texts.contains_key(p) && !self.text_failed.contains(p)).collect()
    }

    /// The whole document's text in page order, one page after another on new lines, from the
    /// pages' text read so far and `more` (pages read for this, keyed by page).
    pub(crate) fn whole_text(&self, more: &HashMap<usize, Arc<PageText>>) -> String {
        let mut out = String::new();
        for p in 0..self.page_count {
            let Some(t) = self.texts.get(&p).or_else(|| more.get(&p)) else { continue };
            let text = t.text_of(0..t.glyphs.len());
            if text.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&text);
        }
        out
    }
}

#[derive(Clone, Copy)]
enum SelectionAction {
    Note,
    Markup(comments::CommentTool),
    Replace,
    Copy,
}

/// The selection bar's width before it has been laid out once (English labels).
const SELECTION_BAR_GUESS: f32 = 520.0;

/// Actions on an existing text selection, next to the selected words.
pub(super) fn selection_actions(
    ui: &mut egui::Ui,
    viewport: Rect,
    view: &mut DocView,
    info: &DocInfo,
    prefs: &comments::CommentPrefs,
    can_comment: bool,
    can_copy: bool,
) -> bool {
    if ui.input(|input| input.pointer.is_decidedly_dragging()) {
        return false;
    }
    let Some(selection) = view.selection else {
        view.actions_dismissed = None;
        return false;
    };
    if view.actions_dismissed == Some(selection) {
        return false;
    }
    let Some(text) = view.texts.get(&selection.page) else { return false };
    let Some(xf) = view.page_xform(selection.page) else { return false };
    let Some(last) = selection.rects(text).last().copied() else { return false };
    let selected = selection.text(text);
    if selected.is_empty() {
        return false;
    }
    let rect = xf.view_rect(last);
    if !viewport.intersects(rect) {
        return false;
    }
    let id = egui::Id::unique(("text-selection-actions", view.view_key()));
    // The bar stays inside the pages' area, by the width it had last frame (its labels make it
    // wider in some languages), so it never covers a side panel; a first guess before it has one.
    let width = egui::AreaState::load(ui.ctx(), id).and_then(|a| a.size).map_or(SELECTION_BAR_GUESS, |s| s.x);
    let x = rect.left().clamp(viewport.left() + 8.0, (viewport.right() - width - 8.0).max(viewport.left() + 8.0));
    let y = if rect.bottom() + 48.0 < viewport.bottom() { rect.bottom() + 6.0 } else { (rect.top() - 44.0).max(viewport.top() + 6.0) };
    let mut chosen = None;
    egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(pos2(x, y)).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (label, action) in [
                    (tl!("Add comment"), SelectionAction::Note),
                    (tl!("Highlight"), SelectionAction::Markup(comments::CommentTool::Highlight)),
                    (tl!("Underline"), SelectionAction::Markup(comments::CommentTool::Underline)),
                    (tl!("Strikethrough"), SelectionAction::Markup(comments::CommentTool::StrikeOut)),
                    (tl!("Replace text"), SelectionAction::Replace),
                ] {
                    if ui.add_enabled(can_comment, egui::Button::new(label)).clicked() {
                        chosen = Some(action);
                    }
                }
                if ui.add_enabled(can_copy, egui::Button::new(tl!("Copy"))).clicked() {
                    chosen = Some(SelectionAction::Copy);
                }
            });
        });
    });
    let Some(action) = chosen else { return false };
    if let SelectionAction::Copy = action {
        // Copying closes the bar but keeps the words selected, for ⌘C or a second action.
        view.actions_dismissed = Some(selection);
        return true;
    }
    let Some((page, quads)) = view.selection_quads(info) else { return false };
    let last = quads.last().copied().unwrap_or_default();
    let at = [last[2].max(last[6]), last[1].max(last[3])];
    view.clear_selection();
    match action {
        SelectionAction::Note | SelectionAction::Replace => {
            let kind = if matches!(action, SelectionAction::Replace) {
                view.comments.replace = Some(quads);
                comments::ComposerKind::Replace
            } else {
                comments::ComposerKind::TextNote { quads }
            };
            view.comments.composer = Some(comments::Composer { page, at, kind, text: String::new(), focus: true });
        }
        SelectionAction::Markup(tool) => {
            // Every markup button carries a markup tool; anything else adds nothing.
            let Some(kind) = tool.markup() else { return false };
            view.pending_edit = Some(Edit::AddAnnotation(pdfcraft_engine::NewAnnotation {
                page,
                shape: pdfcraft_engine::Shape::TextMarkup { kind, quads },
                style: prefs.style(tool),
                contents: String::new(),
                author: prefs.author.clone(),
            }));
        }
        // Copy is dispatched through the app after the view borrow ends.
        SelectionAction::Copy => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::{DocId, PageXform, ViewDefaults};
    use super::*;
    use egui::vec2;
    use pdfcraft_render::{PageInfo, TextGlyph};

    fn test_doc_info() -> DocInfo {
        DocInfo {
            pages: vec![PageInfo { width: 600.0, height: 800.0, label: "1".into(), crop: [0.0, 0.0, 600.0, 800.0], rotation: 0 }],
            ..DocInfo::default()
        }
    }

    fn two_line_page_text(first_y: f32, second_y: f32) -> PageText {
        let glyph = |c: char, y: f32| TextGlyph {
            text: c.into(),
            rect: [50.0, y, 200.0, y + 20.0],
            quad: [[50.0, y], [200.0, y], [200.0, y + 20.0], [50.0, y + 20.0]],
            direction: [1.0, 0.0],
        };
        PageText { glyphs: vec![glyph('a', first_y), glyph('b', second_y)], line_of: vec![0, 1], space_before: vec![false, false], truncated: false }
    }

    fn run_selection_bar(viewport: Rect, first_y: f32, second_y: f32) -> egui::AreaState {
        let info = test_doc_info();
        let mut view = DocView::new(DocId(1), &info, ViewDefaults::default());
        view.screen_xforms = vec![(0, PageXform { rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0)), rot: 0, pw: 600.0, ph: 800.0 })];
        view.texts.insert(0, Arc::new(two_line_page_text(first_y, second_y)));
        view.select_text(0, 0, 1);

        let ctx = egui::Context::default();
        let prefs = comments::CommentPrefs::default();
        let id = egui::Id::unique(("text-selection-actions", view.view_key()));
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                selection_actions(ui, viewport, &mut view, &info, &prefs, true, true);
            });
            out.textures_delta.clear();
        }
        egui::AreaState::load(&ctx, id).expect("selection bar area should be laid out")
    }

    #[test]
    fn selection_bar_opens_above_first_line_with_compact_icons() {
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0));

        // Room above the first selected line: bar must open above line 0, not below line 1.
        let above = run_selection_bar(viewport, 120.0, 150.0);
        let pos = above.pivot_pos.expect("bar has a position");
        assert!(pos.y < 120.0, "expected bar above first line (y < 120.0), got y = {}", pos.y);

        // Compact icon buttons + "Replace text" should be much narrower than 6 full text buttons.
        let size = above.size.expect("bar has a size");
        assert!(size.x < 320.0, "expected compact icon bar (< 320 px), got width = {}", size.x);

        // No room above the first selected line: bar must fall back below the selection.
        let below = run_selection_bar(viewport, 10.0, 40.0);
        let below_pos = below.pivot_pos.expect("bar has a position");
        assert!(below_pos.y >= 60.0, "expected bar below selection (y >= 60.0), got y = {}", below_pos.y);
    }

    #[test]
    fn selection_bar_pos_prefers_above_and_clamps_horizontally() {
        let viewport = Rect::from_min_size(pos2(100.0, 50.0), vec2(500.0, 600.0));
        let first = Rect::from_min_size(pos2(550.0, 200.0), vec2(120.0, 20.0));
        let last = Rect::from_min_size(pos2(120.0, 230.0), vec2(120.0, 20.0));
        let pos = selection_bar_pos(viewport, first, last, 260.0);
        assert_eq!(pos, pos2(600.0 - 260.0 - 8.0, 200.0 - 44.0));

        // Too close to the top of the viewport: falls back below the last line.
        let top_first = Rect::from_min_size(pos2(80.0, 60.0), vec2(120.0, 20.0));
        let top_last = Rect::from_min_size(pos2(120.0, 90.0), vec2(120.0, 20.0));
        let below = selection_bar_pos(viewport, top_first, top_last, 260.0);
        assert_eq!(below, pos2(120.0, 110.0 + 6.0));
    }
}
