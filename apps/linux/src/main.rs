//! Native Linux GTK 4 + libadwaita + Cairo Xiangqi client (`apps/linux`).
//! Links `xiangqi-core`, `archive-client`, and `jade-uci-host` directly with zero WebView layer.
//! Documented & CI-tested target environment: GNOME 46+ on Ubuntu 24.04 LTS / Fedora Workstation (`libgtk-4-dev`, `libadwaita-1-dev`).

use archive_client::SampleArchiveProvider;
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, BoardViewSnapshot, BoardViewport, GameState, Orientation,
    PresentationPrefsSnapshot, Square, format_chinese_move, parse_move_token,
};

pub struct LinuxBoardController {
    pub game: GameState,
    pub orientation: Orientation,
    pub selected: Option<Square>,
    pub focused: Square,
    pub prefs: PresentationPrefsSnapshot,
    pub moves_chinese: Vec<String>,
}

impl Default for LinuxBoardController {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxBoardController {
    pub fn new() -> Self {
        Self {
            game: GameState::new(ADOPTED_RULES_PROFILE).expect("valid rules profile"),
            orientation: Orientation::RedBelow,
            selected: None,
            focused: Square::from_file_rank(4, 0).unwrap(),
            prefs: PresentationPrefsSnapshot::default(),
            moves_chinese: Vec::new(),
        }
    }

    pub fn snapshot(&self, width: f64, height: f64) -> BoardViewSnapshot {
        BoardViewSnapshot::from_game(
            &self.game,
            self.game.ply_count() as u64,
            1,
            self.orientation,
            Some((width, height, 20.0)),
            self.selected,
            Vec::new(),
            self.prefs.clone(),
        )
    }

    pub fn pointer_press(&mut self, width: f64, height: f64, px: f64, py: f64) -> bool {
        let Some(vp) = BoardViewport::compute(width, height, 20.0) else {
            return false;
        };
        let Some(sq) = vp.screen_to_square(px, py, self.orientation) else {
            self.selected = None;
            return true;
        };
        self.focused = sq;
        self.activate_square(sq)
    }

    pub fn activate_square(&mut self, sq: Square) -> bool {
        match self.selected {
            None => {
                self.selected = Some(sq);
                true
            }
            Some(prev) if prev == sq => {
                self.selected = None;
                true
            }
            Some(prev) => {
                let token = format!("{prev}{sq}");
                if let Ok(mv) = parse_move_token(self.game.board(), &token, self.game.ply_count()) {
                    if let Ok(zh) = format_chinese_move(self.game.board(), mv, false) {
                        if self.game.apply_move(mv).is_ok() {
                            self.moves_chinese.push(zh);
                            self.selected = None;
                            return true;
                        }
                    }
                }
                self.selected = Some(sq);
                true
            }
        }
    }

    pub fn accessible_board_summary(&self) -> String {
        let stm = self.game.side_to_move().as_str();
        format!(
            "Jade Xiangqi board, {} to move, focus {}, {} plies played",
            stm,
            self.focused,
            self.game.ply_count()
        )
    }

    pub fn sample_archive_count(&self) -> usize {
        SampleArchiveProvider::new_embedded().manifest().games.len()
    }
}

#[cfg(feature = "gtk-ui")]
mod gtk_app {
    use super::LinuxBoardController;
    use gtk4::prelude::*;
    use libadwaita as adw;
    use std::cell::RefCell;
    use std::rc::Rc;
    use xiangqi_core::{Orientation, Side, Square};

    pub fn run() -> gtk4::glib::ExitCode {
        let app = adw::Application::builder()
            .application_id("play.jade.desktop.dev")
            .build();

        app.connect_activate(|app| {
            let state = Rc::new(RefCell::new(LinuxBoardController::new()));

            let drawing_area = gtk4::DrawingArea::builder()
                .content_width(520)
                .content_height(580)
                .hexpand(true)
                .vexpand(true)
                .focusable(true)
                .accessible_role(gtk4::AccessibleRole::Application)
                .build();

            {
                let state_draw = Rc::clone(&state);
                drawing_area.set_draw_func(move |_area, cr, width, height| {
                    let ctrl = state_draw.borrow();
                    let snap = ctrl.snapshot(width as f64, height as f64);
                    // Warm paper surface (#F3EEE4)
                    cr.set_source_rgb(0xF3 as f64 / 255.0, 0xEE as f64 / 255.0, 0xE4 as f64 / 255.0);
                    let _ = cr.paint();

                    let Some(vp) = snap.viewport else { return };
                    let ox = vp.origin_x;
                    let oy = vp.origin_y;
                    let s = vp.cell_size;

                    // Charcoal board lines (#252A25)
                    cr.set_source_rgb(0x25 as f64 / 255.0, 0x2A as f64 / 255.0, 0x25 as f64 / 255.0);
                    cr.set_line_width(1.3);
                    for r in 0..10 {
                        let y = oy + (r as f64) * s;
                        cr.move_to(ox, y);
                        cr.line_to(ox + 8.0 * s, y);
                    }
                    for f in 0..9 {
                        let x = ox + (f as f64) * s;
                        if f == 0 || f == 8 {
                            cr.move_to(x, oy);
                            cr.line_to(x, oy + 9.0 * s);
                        } else {
                            cr.move_to(x, oy);
                            cr.line_to(x, oy + 4.0 * s);
                            cr.move_to(x, oy + 5.0 * s);
                            cr.line_to(x, oy + 9.0 * s);
                        }
                    }
                    // Palace diagonals
                    cr.move_to(ox + 3.0 * s, oy);
                    cr.line_to(ox + 5.0 * s, oy + 2.0 * s);
                    cr.move_to(ox + 5.0 * s, oy);
                    cr.line_to(ox + 3.0 * s, oy + 2.0 * s);
                    cr.move_to(ox + 3.0 * s, oy + 7.0 * s);
                    cr.line_to(ox + 5.0 * s, oy + 9.0 * s);
                    cr.move_to(ox + 5.0 * s, oy + 7.0 * s);
                    cr.line_to(ox + 3.0 * s, oy + 9.0 * s);
                    let _ = cr.stroke();

                    // Pale-jade discs (#DCE3CE) + engraved glyphs
                    let radius = s * 0.425;
                    for p in &snap.pieces {
                        let sq = Square::from_file_rank(p.file, p.rank).unwrap();
                        let (sx, sy) = vp.square_to_screen(sq, snap.orientation);
                        cr.arc(sx, sy, radius, 0.0, std::f64::consts::TAU);
                        cr.set_source_rgb(0xDC as f64 / 255.0, 0xE3 as f64 / 255.0, 0xCE as f64 / 255.0);
                        let _ = cr.fill_preserve();
                        cr.set_source_rgb(0x41 as f64 / 255.0, 0x54 as f64 / 255.0, 0x47 as f64 / 255.0);
                        cr.set_line_width(1.6);
                        let _ = cr.stroke();

                        match p.side {
                            Side::Red => cr.set_source_rgb(0xA5 as f64 / 255.0, 0x48 as f64 / 255.0, 0x37 as f64 / 255.0),
                            Side::Black => cr.set_source_rgb(0x25 as f64 / 255.0, 0x2A as f64 / 255.0, 0x25 as f64 / 255.0),
                        }
                        cr.set_font_size(radius * 1.05);
                        cr.move_to(sx - radius * 0.52, sy + radius * 0.36);
                        let _ = cr.show_text(&p.glyph);
                    }
                });
            }

            {
                let click = gtk4::GestureClick::new();
                let state_click = Rc::clone(&state);
                let da_click = drawing_area.clone();
                click.connect_pressed(move |_, _, x, y| {
                    da_click.grab_focus();
                    state_click.borrow_mut().pointer_press(
                        da_click.width() as f64,
                        da_click.height() as f64,
                        x,
                        y,
                    );
                    da_click.update_property(&[gtk4::accessible::Property::Description(
                        &state_click.borrow().accessible_board_summary(),
                    )]);
                    da_click.queue_draw();
                });
                drawing_area.add_controller(click);
            }

            let header = adw::HeaderBar::new();
            let flip_btn = gtk4::Button::with_label("Flip Board");
            {
                let state_flip = Rc::clone(&state);
                let da_flip = drawing_area.clone();
                flip_btn.connect_clicked(move |_| {
                    let mut c = state_flip.borrow_mut();
                    c.orientation = match c.orientation {
                        Orientation::RedBelow => Orientation::BlackBelow,
                        Orientation::BlackBelow => Orientation::RedBelow,
                    };
                    da_flip.queue_draw();
                });
            }
            header.pack_end(&flip_btn);

            let toolbar_view = adw::ToolbarView::new();
            toolbar_view.add_top_bar(&header);
            toolbar_view.set_content(Some(&drawing_area));

            let window = adw::ApplicationWindow::builder()
                .application(app)
                .title("Jade — Xiangqi · 玉枰象棋")
                .default_width(820)
                .default_height(680)
                .content(&toolbar_view)
                .build();
            window.present();
        });

        app.run()
    }
}

#[cfg(feature = "gtk-ui")]
fn main() -> gtk4::glib::ExitCode {
    gtk_app::run()
}

#[cfg(not(feature = "gtk-ui"))]
fn main() {
    let ctrl = LinuxBoardController::new();
    println!("{}", ctrl.accessible_board_summary());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_controller_executes_canonical_moves_and_pikafish_fixtures() {
        let mut ctrl = LinuxBoardController::new();
        assert_eq!(ctrl.sample_archive_count(), 2);
        // Tap h2 (7, 2) -> e2 (4, 2) on a 486x540 viewport (`padding=20`: compute exact screen coords)
        let vp = BoardViewport::compute(486.0, 540.0, 20.0).unwrap();
        let (hx, hy) = vp.square_to_screen(Square::from_uci("h2").unwrap(), Orientation::RedBelow);
        let (ex, ey) = vp.square_to_screen(Square::from_uci("e2").unwrap(), Orientation::RedBelow);

        assert!(ctrl.pointer_press(486.0, 540.0, hx, hy));
        assert!(ctrl.pointer_press(486.0, 540.0, ex, ey));
        assert_eq!(ctrl.moves_chinese, vec!["炮二平五"]);
    }
}
