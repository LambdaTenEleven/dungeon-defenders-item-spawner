#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod connection;
mod diagnostics;
mod editor;
mod game;
mod number_input;
mod search;

use catalog::{Filter, Kind, Template};
use connection::GameConnection;
use dd_protocol::{AdapterStatus, Command, MAX_QUANTITY, validate_grant};
use diagnostics::DiagnosticHost;
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const WARNING: Color32 = Color32::from_rgb(232, 187, 96);
const ERROR: Color32 = Color32::from_rgb(240, 150, 120);
const CONNECTED: Color32 = Color32::from_rgb(120, 200, 140);

enum Event {
    Progress(String),
    Adapter(Result<(DiagnosticHost, AdapterStatus), String>),
    Game(Result<String, String>),
    Connected(Result<(GameConnection, AdapterStatus), String>),
    Spawned(Result<AdapterStatus, String>),
}

struct Overlay {
    templates: Vec<Template>,
    filter: Filter,
    /// Template indices matching `filter`, rebuilt only when the filter changes.
    shown: Vec<usize>,
    shown_for: Filter,
    /// Requested template path. Selecting from the list sets it; it can also be edited.
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
    show_settings: bool,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    last_process_check: Instant,
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = mpsc::channel();
        let templates = catalog::templates();
        let template = templates
            .iter()
            .find(|item| item.name == "SwordOfEmbers")
            .unwrap_or(&templates[0])
            .path
            .into();
        let filter = Filter::default();
        let mut app = Self {
            shown: filter.apply(&templates),
            shown_for: filter.clone(),
            filter,
            templates,
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
            show_settings: false,
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

    /// The catalog entry for the requested path, if it was not edited to an unknown template.
    fn current(&self) -> Option<&Template> {
        self.templates
            .iter()
            .find(|item| item.path == self.template)
    }

    fn is_weapon(&self) -> bool {
        self.current().is_none_or(|item| item.kind == Kind::Weapon)
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
        egui::Panel::top("top-bar").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status-bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(&self.adapter_status);
            ui.add_space(2.0);
        });
        egui::Panel::left("equipment")
            .resizable(true)
            .default_size(380.0)
            .min_size(260.0)
            .show(ui, |ui| self.browser(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Panel::bottom("spawn-bar").show(ui, |ui| self.spawn_bar(ui));
            egui::ScrollArea::vertical()
                .id_salt("item-details")
                .auto_shrink([false, false])
                .show(ui, |ui| self.details(ui));
        });
    }
}

impl Overlay {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Dungeon Workshop");
            ui.label(RichText::new("DEVELOPMENT PREVIEW").small().color(WARNING));
            ui.separator();
            let (status, color) = if self.connection.is_some() {
                ("Connected", CONNECTED)
            } else if self.game_running {
                ("Game running · not attached", WARNING)
            } else {
                ("Game not running", ui.visuals().weak_text_color())
            };
            // The default fonts have no filled-circle glyph, so paint the status dot.
            let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, color);
            ui.colored_label(color, status);
            if self.busy {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.show_settings, "Settings");
                if ui.checkbox(&mut self.topmost, "Keep on top").changed() {
                    let level = if self.topmost {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    };
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                }
                let connect = if self.connection.is_some() {
                    "Reconnect"
                } else {
                    "Connect to game"
                };
                if ui
                    .add_enabled(!self.busy, egui::Button::new(connect))
                    .clicked()
                {
                    self.connect(ui.ctx().clone());
                }
            });
        });
        if self.show_settings {
            ui.separator();
            self.settings(ui);
        }
        ui.add_space(4.0);
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Game installation");
        ui.horizontal(|ui| {
            let width = ui.available_width() - 86.0;
            ui.add(egui::TextEdit::singleline(&mut self.install_path).desired_width(width));
            if ui
                .add_enabled(!self.busy, egui::Button::new("Check"))
                .clicked()
            {
                self.inspect_game(ui.ctx().clone());
            }
        });
        ui.label(&self.game_status);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let disconnect = egui::Button::new("Disconnect game");
            if self.connection.is_some() && ui.add_enabled(!self.busy, disconnect).clicked() {
                self.connection = None;
                self.can_spawn = false;
                self.adapter_status =
                    "Game disconnected. Adapter stays idle until the game exits.".into();
            }
            let test = egui::Button::new("Run isolated adapter test");
            if ui.add_enabled(!self.busy, test).clicked() {
                self.test_adapter(ui.ctx().clone());
            }
            if self.diagnostic.is_some() && ui.button("Stop test host").clicked() {
                self.diagnostic = None;
                self.adapter_status =
                    "Isolated adapter test stopped. Game is not connected.".into();
            }
        });
        ui.small("Local single-player only. Picked-up items can be saved by the game. Restart the game before replacing an attached adapter DLL.");
    }

    fn browser(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.filter.text)
                .hint_text("Search name or package…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let mut kind = self.filter.kind;
            ui.selectable_value(&mut kind, None, "All");
            ui.selectable_value(&mut kind, Some(Kind::Weapon), "Weapons");
            ui.selectable_value(&mut kind, Some(Kind::Armor), "Armor");
            self.filter.set_kind(kind);
        });
        if let Some(kind) = self.filter.kind {
            let filter = &mut self.filter;
            chips(ui, kind.group_title(), kind.groups(), &mut filter.groups);
            chips(
                ui,
                kind.variant_title(),
                kind.variants(),
                &mut filter.variants,
            );
        } else {
            ui.weak("Choose Weapons or Armor to filter by hero, slot, variant, or material.");
        }
        ui.horizontal(|ui| {
            if self.filter.is_active() && ui.small_button("Clear filters").clicked() {
                self.filter = Filter::default();
            }
            if self.shown_for != self.filter {
                self.shown = self.filter.apply(&self.templates);
                self.shown_for = self.filter.clone();
            }
            let count = format!("{} of {} shown", self.shown.len(), self.templates.len());
            ui.weak(count).on_hover_text(
                "Availability is checked when spawning; not every template has been tested.",
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("equipment-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.shown.is_empty() {
                    ui.weak("No equipment matches. Clear a filter to see more.");
                }
                for &index in &self.shown {
                    let item = &self.templates[index];
                    let selected = self.template == item.path;
                    let row = ui.horizontal(|ui| {
                        let name = ui.selectable_label(selected, item.name);
                        ui.weak(&item.tags);
                        name
                    });
                    if row.inner.on_hover_text(item.path).clicked() {
                        self.template = item.path.into();
                    }
                }
            });
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        match self.current() {
            Some(item) => {
                ui.heading(item.name);
                ui.weak(format!("{} · {}", item.kind.label(), item.tags));
            }
            None => {
                ui.heading("Custom template");
                ui.weak("Not in the bundled catalog. Availability is checked when spawning.");
            }
        }
        ui.label(RichText::new(&self.template).small().monospace().weak());
        egui::CollapsingHeader::new("Edit template path").show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.template)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
        });
        ui.separator();
        let weapon = self.is_weapon();
        self.editor.show(ui, weapon);
    }

    fn spawn_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let weapon = self.is_weapon();
        let valid = validate_grant(&self.template, self.request_quantity())
            .and_then(|()| self.editor.options(weapon).validate());
        if let Err(error) = valid {
            ui.colored_label(ERROR, error);
        }
        let blocked = if self.connection.is_none() {
            "Connect to the game first."
        } else if !self.can_spawn {
            "The game is not ready for item requests."
        } else if self.busy {
            "Wait for the current request to finish."
        } else {
            "Correct the highlighted value first."
        };
        ui.horizontal(|ui| {
            ui.label("Quantity");
            number_input::number_input(ui, "quantity", &mut self.quantity, i32::MIN).on_hover_text(
                format!("Enter a whole number between 1 and {MAX_QUANTITY}."),
            );
            if let Some(cancel) = &self.search_cancel
                && ui.button("Cancel search").clicked()
            {
                cancel.store(true, Ordering::Release);
            }
            let spawn =
                egui::Button::new("Spawn pickup").min_size(egui::vec2(ui.available_width(), 30.0));
            let enabled = self.can_spawn && !self.busy && valid.is_ok();
            if ui
                .add_enabled(enabled, spawn)
                .on_hover_text("Creates equipment near your hero. Pick it up normally.")
                .on_disabled_hover_text(blocked)
                .clicked()
            {
                self.spawn_item(ui.ctx().clone());
            }
        });
        ui.add_space(6.0);
    }
}

/// Toggle chips for one filter row; selecting several matches any of them.
fn chips(
    ui: &mut egui::Ui,
    title: &str,
    options: &[&'static str],
    selected: &mut BTreeSet<&'static str>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.weak(title);
        for &option in options {
            let on = selected.contains(option);
            if ui.selectable_label(on, option).clicked() {
                if on {
                    selected.remove(option);
                } else {
                    selected.insert(option);
                }
            }
        }
    });
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
