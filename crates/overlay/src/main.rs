#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod connection;
mod diagnostics;
mod editor;
mod game;
mod inspection;
mod number_input;
mod search;

use catalog::{Filter, Kind, Template};
use connection::GameConnection;
use dd_catalog::{CatalogSnapshot, ScanProgress};
use dd_protocol::{
    AdapterStatus, Command, ItemCapabilities, MAX_QUANTITY, validate_grant, validate_item_options,
};
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
    ScanProgress {
        generation: u64,
        progress: ScanProgress,
    },
    Scanned {
        generation: u64,
        game_status: String,
        result: Result<CatalogSnapshot, String>,
    },
    Connected(Result<(GameConnection, AdapterStatus), String>),
    Spawned(Result<AdapterStatus, String>),
    Inspected {
        generation: u64,
        result: Result<AdapterStatus, String>,
    },
}

struct Overlay {
    catalog: Option<Arc<CatalogSnapshot>>,
    catalog_path: String,
    scan: ScanState,
    templates: Vec<Template>,
    filter: Filter,
    /// Template indices matching `filter`, rebuilt only when the filter changes.
    shown: Vec<usize>,
    shown_for: Filter,
    /// Requested template path. Selecting from the list sets it; it can also be edited.
    template: String,
    quantity: i32,
    editor: editor::ItemEditor,
    inspection: inspection::InspectionState,
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

/// A cancelled or superseded worker can never publish over a newer installation.
#[derive(Default)]
struct ScanState {
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    progress: String,
    summary: Option<String>,
    failed: bool,
}

impl ScanState {
    fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn begin(&mut self) -> (u64, Arc<AtomicBool>) {
        self.cancel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        self.failed = false;
        self.summary = None;
        self.progress = "Checking installed packages and the local catalog cache…".into();
        (self.generation, cancel)
    }

    fn accepts(&self, generation: u64) -> bool {
        self.generation == generation && self.cancel.is_some()
    }

    fn record_progress(&mut self, generation: u64, progress: ScanProgress) {
        if !self.accepts(generation) {
            return;
        }
        if progress.total > 0 && progress.completed == progress.total {
            self.summary = Some(progress.message.clone());
        }
        self.progress = if progress.total == 0 {
            progress.message
        } else {
            format!(
                "{}/{}: {}",
                progress.completed, progress.total, progress.message
            )
        };
    }
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = mpsc::channel();
        let templates = Vec::new();
        let template = String::new();
        let filter = Filter::default();
        let mut app = Self {
            catalog: None,
            catalog_path: String::new(),
            scan: ScanState::default(),
            shown: filter.apply(&templates),
            shown_for: filter.clone(),
            filter,
            templates,
            template,
            quantity: 1,
            editor: editor::ItemEditor::default(),
            inspection: inspection::InspectionState::default(),
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
            app.scan_catalog(cc.egui_ctx.clone(), false);
        }
        app
    }

    fn scan_catalog(&mut self, context: egui::Context, force: bool) {
        if self.busy {
            return;
        }
        self.disconnect_catalog();
        let (generation, cancel) = self.scan.begin();
        self.game_status = "Checking the game executable…".into();
        self.catalog_path = self.install_path.clone();
        let path = self.catalog_path.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let game_status = game::inspect(Path::new(&path))
                .unwrap_or_else(|e| format!("Could not inspect game: {e}"));
            let result = dd_catalog::scan(Path::new(&path), force, &cancel, |progress| {
                let _ = tx.send(Event::ScanProgress {
                    generation,
                    progress,
                });
                context.request_repaint();
            });
            let _ = tx.send(Event::Scanned {
                generation,
                game_status,
                result,
            });
            context.request_repaint();
        });
    }

    fn disconnect_catalog(&mut self) {
        if let Some(cancel) = &self.search_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.connection = None;
        self.inspection.invalidate();
        self.can_spawn = false;
    }

    fn catalog_ready(&self) -> bool {
        self.scan.cancel.is_none()
            && !self.scan.failed
            && self.catalog_path == self.install_path
            && self
                .catalog
                .as_ref()
                .is_some_and(|catalog| catalog.issues.is_empty() && !catalog.entries.is_empty())
    }

    fn publish_catalog(&mut self, snapshot: CatalogSnapshot) {
        self.disconnect_catalog();
        self.templates = catalog::templates(&snapshot.entries);
        if !self.templates.iter().any(|item| item.path == self.template) {
            self.template = self
                .templates
                .iter()
                .find(|item| item.unavailable_reason.is_none())
                .or_else(|| self.templates.first())
                .map_or_else(String::new, |item| item.path.clone());
        }
        self.shown = self.filter.apply(&self.templates);
        self.shown_for = self.filter.clone();
        self.editor = editor::ItemEditor::default();
        self.catalog = Some(Arc::new(snapshot));
        self.adapter_status = if self.catalog_ready() {
            "Catalog refreshed. Connect to inspect items in the game."
        } else {
            "Catalog available for browsing. Resolve the scan issues before connecting."
        }
        .into();
    }

    fn test_adapter(&mut self, context: egui::Context) {
        self.connection = None;
        self.inspection.invalidate();
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
        if !self.catalog_ready() {
            return;
        }
        let catalog = self.catalog.as_ref().unwrap().key.clone();
        self.diagnostic = None;
        self.connection = None;
        self.inspection.invalidate();
        self.can_spawn = false;
        self.busy = true;
        self.adapter_status =
            "Connecting and checking the game. The first connection may briefly pause gameplay…"
                .into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Connected(GameConnection::start(catalog)));
            context.request_repaint();
        });
    }

    /// The catalog entry for the requested path, if it was not edited to an unknown template.
    fn current(&self) -> Option<&Template> {
        self.templates
            .iter()
            .find(|item| item.path == self.template)
    }

    fn capabilities(&self) -> Option<ItemCapabilities> {
        let item = self.current()?;
        Some(
            self.inspection
                .result
                .as_ref()
                .filter(|result| result.template == self.template)
                .map_or(item.capabilities, |result| result.capabilities),
        )
    }

    fn sync_selection(&mut self) {
        self.inspection.select(&self.template);
        if let Some(item) = self.current() {
            let kind = item.kind;
            let caps = self.capabilities().unwrap();
            self.editor.select(kind, caps);
        }
    }

    fn inspect_selection(&mut self, context: egui::Context) {
        if self.busy
            || !self.catalog_ready()
            || self
                .current()
                .is_none_or(|item| item.kind == Kind::Unknown || item.unavailable_reason.is_some())
        {
            return;
        }
        let Some(connection) = &self.connection else {
            return;
        };
        let pid = connection.pid;
        let catalog = connection.catalog.clone();
        let Some((generation, template)) = self.inspection.begin(Instant::now()) else {
            return;
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = connection::request(pid, Command::InspectItem { catalog, template });
            let _ = tx.send(Event::Inspected { generation, result });
            context.request_repaint();
        });
    }

    fn request_quantity(&self) -> u8 {
        u8::try_from(self.quantity).unwrap_or(0)
    }

    fn spawn_item(&mut self, context: egui::Context) {
        let Some(connection) = &self.connection else {
            return;
        };
        if self.busy
            || !self.catalog_ready()
            || !self.can_spawn
            || !self.inspection.available(&self.template)
        {
            return;
        }
        let Some(item) = self.current() else {
            return;
        };
        let options = self.editor.options(item.kind, self.capabilities().unwrap());
        if validate_grant(&self.template, self.request_quantity())
            .and_then(|()| validate_item_options(&item.entry, &options))
            .is_err()
        {
            return;
        }
        let pid = connection.pid;
        let command = Command::Spawn {
            catalog: connection.catalog.clone(),
            template: self.template.clone(),
            quantity: self.request_quantity(),
            options,
        };
        self.busy = true;
        self.adapter_status = "Spawning pickup…".into();
        let cancel = Arc::new(AtomicBool::new(false));
        if let Command::Spawn { options, .. } = &command
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
        self.scan.cancel();
        if let Some(cancel) = &self.search_cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::ScanProgress {
                    generation,
                    progress,
                } => {
                    self.scan.record_progress(generation, progress);
                    continue;
                }
                Event::Scanned {
                    generation,
                    game_status,
                    result,
                } => {
                    if self.scan.accepts(generation) {
                        self.scan.cancel = None;
                        self.game_status = game_status;
                        match result {
                            Ok(snapshot) => {
                                self.scan.progress = if snapshot.issues.is_empty()
                                    && snapshot.entries.is_empty()
                                {
                                    "No item candidates were discovered in this installation. Spawning is disabled.".into()
                                } else if snapshot.issues.is_empty() {
                                    self.scan.summary.take().unwrap_or_else(|| {
                                        format!(
                                            "Local catalog ready: {} items from {} packages.",
                                            snapshot.entries.len(),
                                            snapshot.packages.len()
                                        )
                                    })
                                } else {
                                    format!(
                                        "Incomplete catalog: {} items, {} package issues. Spawning is disabled. {}",
                                        snapshot.entries.len(),
                                        snapshot.issues.len(),
                                        self.scan.summary.take().unwrap_or_default()
                                    )
                                };
                                self.publish_catalog(snapshot);
                            }
                            Err(error) => {
                                self.scan.failed = true;
                                self.scan.progress = format!("Catalog scan failed: {error}.");
                                if self.catalog.is_some() {
                                    self.scan.progress.push_str(
                                        " Previous entries remain available for browsing only.",
                                    );
                                }
                            }
                        }
                    }
                    continue;
                }
                _ => {}
            }
            if let Event::Inspected { generation, result } = event {
                let ready = result.as_ref().ok().map(|status| status.can_give);
                let result = result.and_then(|status| {
                    status
                        .inspection
                        .ok_or_else(|| "Adapter omitted selected item inspection.".into())
                });
                if self.inspection.complete(generation, result)
                    && let Some(ready) = ready
                {
                    self.can_spawn = ready;
                }
                continue;
            }
            if let Event::Progress(message) = event {
                self.adapter_status = message;
                continue;
            }
            self.busy = false;
            match event {
                Event::Progress(_)
                | Event::Inspected { .. }
                | Event::ScanProgress { .. }
                | Event::Scanned { .. } => unreachable!(),
                Event::Adapter(Ok((host, status))) => {
                    self.adapter_status = status.message;
                    self.diagnostic = Some(host);
                }
                Event::Adapter(Err(error)) => self.adapter_status = error,
                Event::Connected(Ok((connection, status))) => {
                    self.can_spawn = status.can_give;
                    self.adapter_status = status.message;
                    self.connection = Some(connection);
                    self.inspection.invalidate();
                }
                Event::Connected(Err(error)) => self.adapter_status = error,
                Event::Spawned(Ok(status)) => {
                    self.search_cancel = None;
                    self.inspection.invalidate();
                    self.can_spawn = status.can_give;
                    self.adapter_status = status.message;
                }
                Event::Spawned(Err(error)) => {
                    self.search_cancel = None;
                    self.inspection.invalidate();
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
                self.inspection.invalidate();
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
        self.sync_selection();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Panel::bottom("spawn-bar").show(ui, |ui| self.spawn_bar(ui));
            egui::ScrollArea::vertical()
                .id_salt("item-details")
                .auto_shrink([false, false])
                .show(ui, |ui| self.details(ui));
        });
        self.inspect_selection(ui.ctx().clone());
    }
}

impl Overlay {
    fn catalog_status(&mut self, ui: &mut egui::Ui) {
        if !self.scan.progress.is_empty() {
            ui.label(&self.scan.progress);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.install_path.is_empty(),
                    egui::Button::new("Rebuild catalog"),
                )
                .clicked()
            {
                self.scan_catalog(ui.ctx().clone(), true);
            }
            if self.scan.cancel.is_some() && ui.button("Cancel scan").clicked() {
                self.scan.cancel();
                self.scan.failed = true;
                self.scan.progress = "Scan cancelled. Scan again before connecting.".into();
                if self.catalog.is_some() {
                    self.scan
                        .progress
                        .push_str(" Previous entries are available for browsing.");
                }
            }
        });
        if let Some(catalog) = &self.catalog {
            if !catalog.issues.is_empty() {
                egui::CollapsingHeader::new("Package scan issues").show(ui, |ui| {
                    for issue in &catalog.issues {
                        ui.colored_label(WARNING, issue);
                    }
                });
            }
            if self.catalog_path != self.install_path
                || self.scan.failed
                || self.scan.cancel.is_some()
            {
                ui.colored_label(WARNING, "Displayed entries are not validated for spawning. Refresh the selected installation.");
            }
        }
    }

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
            if self.busy || self.scan.cancel.is_some() {
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
                    .add_enabled(
                        !self.busy && self.catalog_ready(),
                        egui::Button::new(connect),
                    )
                    .on_disabled_hover_text(
                        "A complete catalog for the selected installation is required.",
                    )
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
            if ui
                .add_enabled(
                    !self.busy,
                    egui::TextEdit::singleline(&mut self.install_path).desired_width(width),
                )
                .changed()
            {
                self.scan.cancel();
                self.scan.failed = true;
                self.scan.progress =
                    "Installation changed. Scan this folder before connecting.".into();
                self.disconnect_catalog();
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Scan"))
                .clicked()
            {
                self.scan_catalog(ui.ctx().clone(), false);
            }
        });
        ui.label(&self.game_status);
        self.catalog_status(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let disconnect = egui::Button::new("Disconnect game");
            if self.connection.is_some() && ui.add_enabled(!self.busy, disconnect).clicked() {
                self.connection = None;
                self.inspection.invalidate();
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
        if !self.catalog_ready() {
            self.catalog_status(ui);
            ui.separator();
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.filter.text)
                .hint_text("Search name or package…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let mut kind = self.filter.kind;
            ui.selectable_value(&mut kind, None, "All");
            for option in catalog::KINDS {
                ui.selectable_value(&mut kind, Some(option), option.label());
            }
            self.filter.set_kind(kind);
        });
        if let Some(kind) = self.filter.kind {
            let filter = &mut self.filter;
            chips(
                ui,
                catalog::group_title(kind),
                &catalog::groups(kind, &self.templates),
                &mut filter.groups,
            );
            let variants = catalog::variants(kind);
            if !variants.is_empty() {
                chips(
                    ui,
                    if kind == Kind::Armor {
                        "Material"
                    } else {
                        "Variant"
                    },
                    &variants,
                    &mut filter.variants,
                );
            }
        } else {
            ui.weak("Choose a category to filter by hero, slot, or item family.");
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
                "Discovered items are inspected before spawning. Unknown item types remain visible and unavailable.",
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("equipment-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.shown.is_empty() {
                    ui.weak(if self.templates.is_empty() {
                        "Scan the game installation to discover items."
                    } else {
                        "No items match. Clear a filter to see more."
                    });
                }
                for &index in &self.shown {
                    let item = &self.templates[index];
                    let selected = self.template == item.path;
                    let row = ui.horizontal(|ui| {
                        let name = ui.selectable_label(selected, &item.name);
                        ui.weak(&item.tags);
                        if let Some(reason) = &item.unavailable_reason {
                            ui.colored_label(WARNING, "Unavailable")
                                .on_hover_text(reason);
                        }
                        name
                    });
                    if row.inner.on_hover_text(&item.path).clicked() {
                        self.template = item.path.clone();
                    }
                }
            });
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        if self.templates.is_empty() {
            ui.heading("Item catalog");
            ui.weak("Select the Dungeon Defenders installation in Settings and complete a scan to browse its items.");
            return;
        }
        match self.current() {
            Some(item) => {
                ui.heading(&item.name);
                ui.weak(format!("{} · {}", item.kind.label(), item.tags));
            }
            None => {
                ui.heading("Unknown template");
                ui.colored_label(
                    ERROR,
                    "This path is not in the shared catalog and cannot be spawned.",
                );
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
        self.sync_selection();
        ui.separator();
        if let Some(item) = self.current() {
            let kind = item.kind;
            let caps = self.capabilities().unwrap();
            let excluded = item.unavailable_reason.clone().or_else(|| {
                (kind == Kind::Unknown)
                    .then(|| "This item type is not classified and cannot be spawned.".into())
            });
            egui::CollapsingHeader::new("Template source").show(ui, |ui| {
                ui.label(format!("Class: {}", item.class_name));
                ui.label(format!("Packages: {}", item.source_packages.join(", ")));
                ui.label(format!("Classification: {}", item.classification_rule));
            });
            let options_available = excluded.is_none() && kind != Kind::Unknown;
            if let Some(reason) = excluded {
                ui.colored_label(WARNING, reason);
            } else if self.connection.is_some() {
                let color = if self.inspection.available(&self.template) {
                    CONNECTED
                } else {
                    WARNING
                };
                ui.colored_label(color, self.inspection.reason());
                if ui
                    .add_enabled(
                        !self.busy && !self.inspection.pending(),
                        egui::Button::new("Recheck availability"),
                    )
                    .clicked()
                {
                    self.inspection.invalidate();
                }
            } else {
                ui.weak("Connect to inspect this item in the current game context.");
            }
            ui.add_space(6.0);
            if options_available {
                self.editor.show(ui, kind, caps);
            }
        }
    }

    fn spawn_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        if self.templates.is_empty() {
            ui.weak("Spawning is unavailable until the catalog contains a supported item.");
            return;
        }
        let valid = validate_grant(&self.template, self.request_quantity()).and_then(|()| {
            let item = self
                .current()
                .ok_or("Select an item from the shared catalog.")?;
            validate_item_options(
                &item.entry,
                &self.editor.options(item.kind, self.capabilities().unwrap()),
            )
        });
        if let Err(error) = valid {
            ui.colored_label(ERROR, error);
        }
        let inspected = self.inspection.available(&self.template);
        let blocked = if !self.catalog_ready() {
            "Scan the selected installation successfully before spawning."
        } else if self.connection.is_none() {
            "Connect to the game first."
        } else if !self.can_spawn {
            "The game is not ready for item requests."
        } else if self.busy {
            "Wait for the current request to finish."
        } else if !inspected {
            self.inspection.reason()
        } else {
            "Correct the highlighted value first."
        }
        .to_owned();
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
            let enabled = self.connection.is_some()
                && self.catalog_ready()
                && self.can_spawn
                && !self.busy
                && inspected
                && valid.is_ok();
            if ui
                .add_enabled(enabled, spawn)
                .on_hover_text("Creates the selected pickup near your hero. Collect it normally.")
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
fn chips(ui: &mut egui::Ui, title: &str, options: &[String], selected: &mut BTreeSet<String>) {
    ui.horizontal_wrapped(|ui| {
        ui.weak(title);
        for option in options {
            let on = selected.contains(option);
            if ui.selectable_label(on, option).clicked() {
                if on {
                    selected.remove(option);
                } else {
                    selected.insert(option.clone());
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

#[cfg(test)]
mod scan_tests {
    use super::*;

    #[test]
    fn changed_installation_rejects_old_scan_results() {
        let mut state = ScanState::default();
        let (old, old_cancel) = state.begin();
        let (new, _) = state.begin();
        assert!(old_cancel.load(Ordering::Acquire));
        assert!(!state.accepts(old));
        assert!(state.accepts(new));
    }

    #[test]
    fn cancelled_scan_cannot_publish_or_update_progress() {
        let mut state = ScanState::default();
        let (generation, cancel) = state.begin();
        state.cancel();
        assert!(cancel.load(Ordering::Acquire));
        assert!(!state.accepts(generation));
        assert!(state.cancel.is_none());
    }

    #[test]
    fn completion_summary_survives_and_stale_progress_cannot_replace_it() {
        let mut state = ScanState::default();
        let (old, _) = state.begin();
        let (current, _) = state.begin();
        state.record_progress(
            current,
            ScanProgress {
                completed: 3,
                total: 3,
                message: "2 cached packages reused, 1 reparsed".into(),
            },
        );
        state.record_progress(
            old,
            ScanProgress {
                completed: 1,
                total: 1,
                message: "Wrong installation".into(),
            },
        );
        assert_eq!(
            state.summary.as_deref(),
            Some("2 cached packages reused, 1 reparsed")
        );
        assert!(!state.progress.contains("Wrong installation"));
    }
}
