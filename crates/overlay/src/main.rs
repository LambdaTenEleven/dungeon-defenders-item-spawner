#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod connection;
mod diagnostics;
mod editor;
mod game;
mod number_input;
mod search;

use connection::GameConnection;
use dd_protocol::{AdapterStatus, Command, MAX_QUANTITY, validate_grant};
use diagnostics::DiagnosticHost;
use eframe::egui::{self, Color32, RichText};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Template {
    name: &'static str,
    kind: &'static str,
    path: &'static str,
}

fn templates() -> Vec<Template> {
    include_str!("equipment.tsv")
        .lines()
        .filter_map(|line| {
            let (kind, path) = line.split_once('\t')?;
            let leaf = path.rsplit('.').next()?;
            let name = leaf.strip_prefix("HeroEquipment_").unwrap_or(leaf);
            Some(Template { name, kind, path })
        })
        .collect()
}

enum Event {
    Progress(String),
    Adapter(Result<(DiagnosticHost, AdapterStatus), String>),
    Game(Result<String, String>),
    Connected(Result<(GameConnection, AdapterStatus), String>),
    Spawned(Result<AdapterStatus, String>),
}

struct Overlay {
    templates: Vec<Template>,
    search: String,
    selected: usize,
    template: String,
    quantity: i32,
    editor: editor::ItemEditor,
    install_path: String,
    game_status: String,
    game_running: bool,
    adapter_status: String,
    diagnostic: Option<DiagnosticHost>,
    connection: Option<GameConnection>,
    can_spawn: bool,
    busy: bool,
    search_cancel: Option<Arc<AtomicBool>>,
    topmost: bool,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    last_process_check: Instant,
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = mpsc::channel();
        let templates = templates();
        let selected = templates
            .iter()
            .position(|item| item.name == "SwordOfEmbers")
            .unwrap_or(0);
        let template = templates[selected].path.into();
        let mut app = Self {
            templates,
            search: String::new(),
            selected,
            template,
            quantity: 1,
            editor: editor::ItemEditor::default(),
            install_path: game::find_installation()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            game_status: "Select your Dungeon Defenders folder.".into(),
            game_running: game::running(),
            adapter_status: "Start local single-player and connect to the game.".into(),
            diagnostic: None,
            connection: None,
            can_spawn: false,
            busy: false,
            search_cancel: None,
            topmost: true,
            tx,
            rx,
            last_process_check: Instant::now(),
        };
        if !app.install_path.is_empty() {
            app.inspect_game(cc.egui_ctx.clone());
        }
        app
    }

    fn inspect_game(&mut self, context: egui::Context) {
        self.busy = true;
        self.game_status = "Checking the game executable…".into();
        let path = self.install_path.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = game::inspect(Path::new(&path)).map_err(|e| e.to_string());
            let _ = tx.send(Event::Game(result));
            context.request_repaint();
        });
    }

    fn test_adapter(&mut self, context: egui::Context) {
        self.connection = None;
        self.can_spawn = false;
        self.diagnostic = None;
        self.busy = true;
        self.adapter_status = "Starting the isolated 32-bit adapter test…".into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Adapter(DiagnosticHost::start()));
            context.request_repaint();
        });
    }

    fn connect(&mut self, context: egui::Context) {
        self.diagnostic = None;
        self.connection = None;
        self.can_spawn = false;
        self.busy = true;
        self.adapter_status =
            "Connecting and checking the game. The first connection may briefly pause gameplay…"
                .into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Connected(GameConnection::start()));
            context.request_repaint();
        });
    }

    fn is_weapon(&self) -> bool {
        self.templates
            .iter()
            .find(|item| item.path == self.template)
            .is_none_or(|item| item.kind == "Weapon")
    }

    fn request_quantity(&self) -> u8 {
        u8::try_from(self.quantity).unwrap_or(0)
    }

    fn spawn_item(&mut self, context: egui::Context) {
        let Some(connection) = &self.connection else {
            return;
        };
        let pid = connection.pid;
        let command = Command::Give {
            template: self.template.clone(),
            quantity: self.request_quantity(),
            options: self.editor.options(self.is_weapon()),
        };
        self.busy = true;
        self.adapter_status = "Spawning pickup…".into();
        let cancel = Arc::new(AtomicBool::new(false));
        if let Command::Give { options, .. } = &command
            && options.needs_search()
        {
            self.search_cancel = Some(Arc::clone(&cancel));
            self.adapter_status = "Searching natural rolls…".into();
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = search::run(pid, command, &cancel, |message| {
                let _ = tx.send(Event::Progress(message));
                context.request_repaint();
            });
            let _ = tx.send(Event::Spawned(result));
            context.request_repaint();
        });
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if let Some(cancel) = &self.search_cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            if let Event::Progress(message) = event {
                self.adapter_status = message;
                continue;
            }
            self.busy = false;
            match event {
                Event::Progress(_) => unreachable!(),
                Event::Game(result) => {
                    self.game_status =
                        result.unwrap_or_else(|e| format!("Could not inspect game: {e}"))
                }
                Event::Adapter(Ok((host, status))) => {
                    self.adapter_status = status.message;
                    self.diagnostic = Some(host);
                }
                Event::Adapter(Err(error)) => self.adapter_status = error,
                Event::Connected(Ok((connection, status))) => {
                    self.can_spawn = status.can_give;
                    self.adapter_status = status.message;
                    self.connection = Some(connection);
                }
                Event::Connected(Err(error)) => self.adapter_status = error,
                Event::Spawned(Ok(status)) => {
                    self.search_cancel = None;
                    self.can_spawn = status.can_give;
                    self.adapter_status = status.message;
                }
                Event::Spawned(Err(error)) => {
                    self.search_cancel = None;
                    self.adapter_status = error;
                    self.can_spawn = false;
                }
            }
        }
        if self.last_process_check.elapsed() > Duration::from_secs(2) {
            self.game_running = game::running();
            self.last_process_check = Instant::now();
            if self
                .connection
                .as_mut()
                .is_some_and(|connection| !connection.is_running())
            {
                self.connection = None;
                if let Some(cancel) = &self.search_cancel {
                    cancel.store(true, Ordering::Release);
                }
                self.can_spawn = false;
                self.adapter_status =
                    "Game attachment ended. Connect again when the game is ready.".into();
            }
            if self
                .diagnostic
                .as_mut()
                .is_some_and(|host| !host.is_running())
            {
                self.diagnostic = None;
                self.adapter_status = "Isolated adapter test stopped. You can run it again.".into();
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        egui::CentralPanel::default().show(ui, |ui| {
            let equipment_height = (ui.available_height() - 330.0).max(140.0);
            egui::ScrollArea::vertical().id_salt("main-panel").show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("Dungeon Workshop");
                ui.label(RichText::new("DEVELOPMENT PREVIEW").small().color(Color32::from_rgb(232, 187, 96)));
            });
            ui.label("Equipment tools for Dungeon Defenders single-player");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(if self.connection.is_some() { "● Game adapter connected" } else if self.game_running { "● Game running · not attached" } else { "○ Game not running" });
                ui.separator();
                if ui.checkbox(&mut self.topmost, "Keep on top").changed() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.topmost {
                        egui::WindowLevel::AlwaysOnTop
                    } else { egui::WindowLevel::Normal }));
                }
                if self.busy { ui.spinner(); }
            });
            ui.separator();
            ui.add_space(8.0);
            ui.columns(2, |columns| {
                let left = &mut columns[0];
                left.heading("Equipment");
                left.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search weapons, armor, or package…").desired_width(f32::INFINITY));
                left.add_space(8.0);
                let search = self.search.to_lowercase();
                egui::ScrollArea::vertical().max_height(equipment_height).auto_shrink([false, false]).show(left, |ui| {
                for (index, item) in self.templates.iter().enumerate() {
                    if !format!("{} {}", item.path, item.kind).to_lowercase().contains(&search) { continue; }
                    if ui.selectable_label(self.selected == index, format!("{}  ·  {}", item.name, item.kind)).on_hover_text(item.path).clicked() {
                        self.selected = index;
                        self.template = item.path.into();
                    }
                }
                });
                left.add_space(16.0);
                left.label(RichText::new(format!("{} weapon and armor references. Availability is checked when spawning; not every template has been tested.", self.templates.len())).small().weak());

                let right = &mut columns[1];
                right.heading("Item request");
                right.label("Template path");
                right.add(egui::TextEdit::multiline(&mut self.template).desired_rows(3).desired_width(f32::INFINITY));
                right.horizontal(|ui| {
                    ui.label("Quantity");
                    number_input::number_input(ui, "quantity", &mut self.quantity, i32::MIN)
                        .on_hover_text(format!("Enter a whole number between 1 and {MAX_QUANTITY}."));
                });
                right.add_space(8.0);
                let weapon = self.is_weapon();
                self.editor.show(right, weapon, (equipment_height - 145.0).max(80.0));
                let valid = validate_grant(&self.template, self.request_quantity()).and_then(|()| self.editor.options(weapon).validate());
                if let Err(error) = valid {
                    right.colored_label(Color32::from_rgb(240, 150, 120), error);
                }
                right.add_space(10.0);
                right.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.can_spawn && !self.busy && valid.is_ok(), egui::Button::new("Spawn pickup").min_size(egui::vec2(ui.available_width(), 34.0))).clicked() { self.spawn_item(ui.ctx().clone()); }
                });
                if let Some(cancel) = &self.search_cancel
                    && right.button("Cancel search").clicked() { cancel.store(true, Ordering::Release); }
                right.label("Creates equipment near your hero with the selected settings. Pick it up normally to equip or store it.");
            });
            ui.add_space(16.0);
            ui.separator();
            egui::CollapsingHeader::new("Game installation").default_open(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.install_path).desired_width(ui.available_width() - 86.0));
                    if ui.add_enabled(!self.busy, egui::Button::new("Check")).clicked() { self.inspect_game(ui.ctx().clone()); }
                });
                ui.label(&self.game_status);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy, egui::Button::new(if self.connection.is_some() { "Reconnect to game" } else { "Connect to game" })).clicked() { self.connect(ui.ctx().clone()); }
                if self.connection.is_some() && ui.add_enabled(!self.busy, egui::Button::new("Disconnect game")).clicked() {
                    self.connection = None;
                    self.can_spawn = false;
                    self.adapter_status = "Game disconnected. Adapter stays idle until the game exits.".into();
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy, egui::Button::new("Run isolated adapter test")).clicked() {
                    self.test_adapter(ui.ctx().clone());
                }
                if self.diagnostic.is_some() && ui.button("Stop test host").clicked() {
                    self.diagnostic = None;
                    self.adapter_status = "Isolated adapter test stopped. Game is not connected.".into();
                }
            });
            ui.label(&self.adapter_status);
            ui.add_space(6.0);
            ui.label(RichText::new("Local single-player only. Picked-up items can be saved by the game. Restart the game before replacing an attached adapter DLL.").small().weak());
            });
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--self-test") => return diagnostics::self_test().map_err(Into::into),
        Some("--check-game") => {
            let path = args
                .next()
                .ok_or("Usage: dd-overlay.exe --check-game <game-folder>")?;
            println!("{}", game::inspect(Path::new(&path))?);
            return Ok(());
        }
        Some(_) => {
            return Err("Unknown argument. Use --self-test or --check-game <game-folder>.".into());
        }
        None => {}
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 600.0])
            .with_min_inner_size([640.0, 420.0])
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Dungeon Workshop",
        options,
        Box::new(|cc| Ok(Box::new(Overlay::new(cc)))),
    )?;
    Ok(())
}
