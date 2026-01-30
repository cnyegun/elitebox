use eframe::egui;
use std::path::{PathBuf, Path};
use std::sync::{Arc, Mutex, mpsc};

pub enum GuiMessage {
    AddToPlaylist(PathBuf),
}

pub struct PlayerState {
    pub current_track: Option<TrackInfo>,
    pub is_playing: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub volume_db: f64,
    pub playlist: Vec<PathBuf>,
    pub command: Option<PlayerCommand>,
    pub error_message: Option<String>,
    pub album_art: Option<Vec<u8>>,
}

#[derive(PartialEq, Clone)]
pub enum PlayerCommand {
    Next,
    Prev,
    PlayIndex(usize),
}

#[derive(PartialEq, Clone)]
pub struct TrackInfo {
    pub filename: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub struct SucklessPlayer {
    tx: mpsc::Sender<GuiMessage>,
    player: Arc<Mutex<PlayerState>>,
    current_dir: PathBuf,
    files: Vec<PathBuf>,
    current_track: usize,
    selected_idx: usize,
    dragging_path: Option<PathBuf>,
}

impl SucklessPlayer {
    pub fn new(tx: mpsc::Sender<GuiMessage>, player: Arc<Mutex<PlayerState>>) -> Self {
        let mut player = Self {
            tx,
            player,
            current_dir: PathBuf::from("."),
            files: Vec::new(),
            current_track: 0,
            selected_idx: 0,
            dragging_path: None,
        };
        player.refresh_files();
        player
    }

    fn setup_fonts(&self, ctx: &egui::Context) {
        egui_extras::install_image_loaders(ctx);
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Inter".to_owned(),
            egui::FontData::from_static(include_bytes!("../../assets/Inter-Regular.ttf")),
        );
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "Inter".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "Inter".to_owned());
        ctx.set_fonts(fonts);
    }

    fn refresh_files(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.current_dir) {
            let mut new_files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();
            new_files.sort_by(|a, b| {
                if a.is_dir() != b.is_dir() { b.is_dir().cmp(&a.is_dir()) } 
                else { a.file_name().cmp(&b.file_name()) }
            });
            self.files = new_files;
        }
    }

    fn apply_suckless_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        use egui::{FontId, TextStyle, FontFamily};
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(16.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Proportional)),
        ].into();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(0xeb, 0xdb, 0xb2));
        style.visuals.panel_fill = egui::Color32::from_rgb(0x1d, 0x20, 0x21);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x28, 0x28, 0x28);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x45, 0x85, 0x88);
        style.visuals.window_rounding = 0.0.into();
        style.visuals.widgets.inactive.rounding = 0.0.into();
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        ctx.set_style(style);
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    if let Some(path) = &file.path {
                        self.add_path_to_playlist_recursive(path);
                    }
                }
            }
        });

        let mut cmd = None;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) { cmd = Some("toggle"); }
            if i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown) { cmd = Some("down"); }
            if i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp) { cmd = Some("up"); }
            if i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::Enter) { cmd = Some("enter"); }
            if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::Backspace) { cmd = Some("back"); }
            if i.key_pressed(egui::Key::N) { cmd = Some("next"); }
            if i.key_pressed(egui::Key::P) { cmd = Some("prev"); }
            if i.key_pressed(egui::Key::S) { cmd = Some("stop"); }
            if i.key_pressed(egui::Key::Q) { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        });
        match cmd {
            Some("toggle") => self.toggle_playback(),
            Some("down") => self.move_selection(1),
            Some("up") => self.move_selection(-1),
            Some("enter") => self.play_selected(),
            Some("back") => self.go_to_parent(),
            Some("next") => self.next(),
            Some("prev") => self.prev(),
            Some("stop") => self.stop(),
            _ => {}
        }
    }

    fn render_transport_controls(&mut self, ui: &mut egui::Ui) {
        let (playing, current_track, position, duration) = {
            let state = self.player.lock().unwrap();
            (state.is_playing, state.current_track.clone(), state.position_secs, state.duration_secs)
        };

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("NOW PLAYING:").strong());
                if let Some(track) = current_track {
                    let display_name = if let (Some(t), Some(a)) = (&track.title, &track.artist) {
                        format!("{} - {}", a, t)
                    } else {
                        track.filename.clone()
                    };
                    ui.label(egui::RichText::new(display_name).color(egui::Color32::from_rgb(0xba, 0xbd, 0x2f)));
                    ui.label(format!("| {}Hz / {}bit", track.sample_rate, track.bit_depth));
                } else { ui.label("[Stopped]"); }
            });

            if playing || (position > 0.0) {
                ui.horizontal(|ui| {
                    let progress = if duration > 0.0 { position / duration } else { 0.0 };
                    ui.add(egui::ProgressBar::new(progress as f32).desired_height(4.0).desired_width(ui.available_width() - 300.0));
                    ui.label(format!("{:.0}s / {:.0}s", position, duration));
                });
            }

            ui.horizontal(|ui| {
                if ui.button("⏮ PREV").clicked() { self.prev(); }
                let btn_text = if playing { "⏸ PAUSE" } else { "▶ PLAY" };
                if ui.button(btn_text).clicked() { self.toggle_playback(); }
                if ui.button("⏹ STOP").clicked() { self.stop(); }
                if ui.button("⏭ NEXT").clicked() { self.next(); }
                
                ui.add_space(20.0);
                ui.label("Volume:");
                let mut state = self.player.lock().unwrap();
                ui.add(egui::Slider::new(&mut state.volume_db, -60.0..=0.0).show_value(true));
            });
        });
    }

    fn render_album_art(&mut self, ui: &mut egui::Ui) {
        let (art, filename) = {
            let state = self.player.lock().unwrap();
            (state.album_art.clone(), state.current_track.as_ref().map(|t| t.filename.clone()))
        };

        if let Some(data) = art {
            let uri = format!("bytes://{}.jpg", filename.unwrap_or_default());
            let image = egui::Image::from_bytes(uri, data)
                .rounding(4.0)
                .fit_to_exact_size(egui::vec2(300.0, 300.0));
            ui.add(image);
        } else {
            // Placeholder
            let (rect, _) = ui.allocate_at_least(egui::vec2(300.0, 300.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(0x28, 0x28, 0x28));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "♫", egui::FontId::proportional(64.0), egui::Color32::from_rgb(0x1d, 0x20, 0x21));
        }
    }

    fn render_file_browser(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("📁 BROWSER")).strong());
        ui.label(egui::RichText::new(format!("{}", self.current_dir.display())).size(12.0).color(egui::Color32::GRAY));
        ui.separator();
        let files = self.files.clone();
        egui::ScrollArea::vertical()
            .id_source("browser")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if ui.selectable_label(false, "⮤ .. (Parent)").clicked() { self.go_to_parent(); }
                for (idx, path) in files.iter().enumerate() {
                    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                    let label = if path.is_dir() { format!("📁 {}", name) } else { format!("♫ {}", name) };
                    
                    let is_selected = self.selected_idx == idx;
                    let response = ui.selectable_label(is_selected, label);
                    
                    if response.drag_started() { self.dragging_path = Some(path.clone()); }
                    if response.clicked() { self.select_and_enter(idx); }
                }
            });
    }

    fn render_playlist(&mut self, ui: &mut egui::Ui) {
        let (playlist, cur_idx) = {
            let state = self.player.lock().unwrap();
            (state.playlist.clone(), self.current_track)
        };
        
        let rect = ui.available_rect_before_wrap();
        
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("PLAYLIST ({})", playlist.len())).strong());
        ui.separator();
        
        egui::ScrollArea::vertical()
            .id_source("playlist")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (idx, path) in playlist.iter().enumerate() {
                    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                    let is_current = idx == cur_idx;
                    let text = if is_current { format!("▶ {}", name) } else { format!("  {}", name) };
                    if ui.selectable_label(is_current, text).clicked() { self.play_index(idx); }
                }
            });

        if ui.input(|i| i.pointer.any_released()) {
            if let Some(path) = self.dragging_path.take() {
                if ui.rect_contains_pointer(rect) {
                    self.add_path_to_playlist_recursive(&path);
                }
            }
        }
    }

    fn toggle_playback(&mut self) { 
        let mut state = self.player.lock().unwrap();
        if !state.playlist.is_empty() { state.is_playing = !state.is_playing; }
    }
    fn move_selection(&mut self, delta: i32) {
        let new_idx = self.selected_idx as i32 + delta;
        if new_idx >= 0 && new_idx < self.files.len() as i32 { self.selected_idx = new_idx as usize; }
    }
    fn play_selected(&mut self) { self.select_and_enter(self.selected_idx); }
    fn go_to_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            self.refresh_files();
            self.selected_idx = 0;
        }
    }
    fn select_and_enter(&mut self, idx: usize) {
        if idx >= self.files.len() { return; }
        let path = self.files[idx].clone();
        if path.is_dir() {
            self.current_dir = path;
            self.refresh_files();
            self.selected_idx = 0;
        } else if is_audio_file(&path) {
            self.tx.send(GuiMessage::AddToPlaylist(path)).unwrap();
        }
    }

    fn add_path_to_playlist_recursive(&self, path: &Path) {
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                entries.sort_by_key(|e| e.path());
                for entry in entries {
                    self.add_path_to_playlist_recursive(&entry.path());
                }
            }
        } else if is_audio_file(path) {
            self.tx.send(GuiMessage::AddToPlaylist(path.to_path_buf())).unwrap();
        }
    }

    fn prev(&mut self) { self.player.lock().unwrap().command = Some(PlayerCommand::Prev); }
    fn next(&mut self) { self.player.lock().unwrap().command = Some(PlayerCommand::Next); }
    fn stop(&mut self) { 
        let mut state = self.player.lock().unwrap();
        state.is_playing = false;
        state.command = Some(PlayerCommand::PlayIndex(0)); 
    }
    fn play_index(&mut self, idx: usize) {
        self.current_track = idx;
        self.player.lock().unwrap().command = Some(PlayerCommand::PlayIndex(idx));
    }
}

impl eframe::App for SucklessPlayer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| { self.setup_fonts(ctx); });
        self.apply_suckless_theme(ctx);
        self.handle_input(ctx);

        // 1. Top Panel: Controls
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            let error = self.player.lock().unwrap().error_message.clone();
            if let Some(msg) = error {
                ui.colored_label(egui::Color32::from_rgb(0xfb, 0x49, 0x34), format!("⚠ {}", msg));
            }
            self.render_transport_controls(ui);
            ui.add_space(4.0);
        });

        // 2. Left Panel: Browser (Fixed height problem)
        egui::SidePanel::left("browser_panel")
            .resizable(true)
            .default_width(450.0)
            .width_range(200.0..=800.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                self.render_album_art(ui);
                ui.add_space(8.0);
                ui.separator();
                self.render_file_browser(ui);
            });

        // 3. Central Panel: Playlist (Fills remaining space)
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_playlist(ui);
        });

        // 4. Drag and Drop Ghost
        if let Some(path) = &self.dragging_path {
            let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            egui::Area::new(egui::Id::new("dnd_ghost"))
                .interactable(false)
                .fixed_pos(ctx.pointer_interact_pos().unwrap_or_default())
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(format!("➕ {}", name))
                        .background_color(egui::Color32::from_rgba_premultiplied(0, 0, 0, 180))
                        .size(20.0));
                });
        }

        ctx.request_repaint();
    }
}

fn is_audio_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|s| s.to_str().map(|s| s.to_lowercase())), 
        Some(ext) if ext == "flac" || ext == "wav" || ext == "mp3" || ext == "aac")
}
