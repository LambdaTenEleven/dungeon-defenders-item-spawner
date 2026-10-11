#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod game;

use dd_protocol::{AdapterStatus, MAX_QUANTITY, validate_grant};
use diagnostics::DiagnosticHost;
use eframe::egui::{self, Color32, RichText};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

struct Template {
    name: &'static str,
    kind: &'static str,
    path: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "Sword of Embers",
        kind: "Weapon",
        path: "DunDef_CDT_Items.SwordOfEmbers.HeroEquipment_SwordOfEmbers",
    },
    Template {
        name: "Magic staff · style 4",
        kind: "Weapon",
        path: "DunDefEquipment.EquipmentMage.MagicStaffStyle4",
    },
    Template {
        name: "Pristine helmet",
        kind: "Armor",
        path: "DunDefEquipment.EquipmentGeneric.HelmetArmorBase_Pristine",
    },
];

enum Event {
    Adapter(Result<(DiagnosticHost, AdapterStatus), String>),
    Game(Result<String, String>),
}

struct Overlay {
    search: String,
    selected: usize,
    template: String,
    quantity: u8,
    install_path: String,
    game_status: String,
    game_running: bool,
    adapter_status: String,
    diagnostic: Option<DiagnosticHost>,
    busy: bool,
    topmost: bool,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    last_process_check: Instant,
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            search: String::new(),
            selected: 0,
            template: TEMPLATES[0].path.into(),
            quantity: 1,
            install_path: game::find_installation()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            game_status: "Select your Dungeon Defenders folder.".into(),
            game_running: game::running(),
            adapter_status: "Not connected".into(),
            diagnostic: None,
            busy: false,
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
        self.diagnostic = None;
        self.busy = true;
        self.adapter_status = "Starting the 32-bit adapter test…".into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Adapter(DiagnosticHost::start()));
            context.request_repaint();
        });
    }
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            self.busy = false;
            match event {
                Event::Game(result) => {
                    self.game_status =
                        result.unwrap_or_else(|e| format!("Could not inspect game: {e}"))
                }
                Event::Adapter(Ok((host, status))) => {
                    self.adapter_status = status.message;
                    self.diagnostic = Some(host);
                }
                Event::Adapter(Err(error)) => self.adapter_status = error,
            }
        }
        if self.last_process_check.elapsed() > Duration::from_secs(2) {
            self.game_running = game::running();
            self.last_process_check = Instant::now();
            if self
                .diagnostic
                .as_mut()
                .is_some_and(|host| !host.is_running())
            {
                self.diagnostic = None;
                self.adapter_status = "Adapter test stopped. You can reconnect.".into();
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("Dungeon Workshop");
                ui.label(RichText::new("DEVELOPMENT PREVIEW").small().color(Color32::from_rgb(232, 187, 96)));
            });
            ui.label("Equipment tools for Dungeon Defenders single-player");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(if self.game_running { "● Game detected" } else { "○ Game not running" });
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
                left.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search starter templates…").desired_width(f32::INFINITY));
                left.add_space(8.0);
                for (index, item) in TEMPLATES.iter().enumerate() {
                    if !format!("{} {}", item.name, item.kind).to_lowercase().contains(&self.search.to_lowercase()) { continue; }
                    if left.selectable_label(self.selected == index, format!("{}  ·  {}", item.name, item.kind)).clicked() {
                        self.selected = index;
                        self.template = item.path.into();
                    }
                }
                left.add_space(16.0);
                left.label(RichText::new("Starter references from the installed game. Availability still needs in-game verification.").small().weak());

                let right = &mut columns[1];
                right.heading("Item request");
                right.label("Template path");
                right.add(egui::TextEdit::multiline(&mut self.template).desired_rows(3).desired_width(f32::INFINITY));
                right.horizontal(|ui| {
                    ui.label("Quantity");
                    ui.add(egui::DragValue::new(&mut self.quantity).range(1..=MAX_QUANTITY));
                });
                if let Err(error) = validate_grant(&self.template, self.quantity) {
                    right.colored_label(Color32::from_rgb(240, 150, 120), error);
                }
                right.add_space(10.0);
                right.add_enabled(false, egui::Button::new("Give item").min_size(egui::vec2(160.0, 34.0)));
                right.label("Item granting is not available in this preview. Native game integration is the next step.");
            });
            ui.add_space(16.0);
            ui.separator();
            egui::CollapsingHeader::new("Game installation").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.install_path).desired_width(ui.available_width() - 86.0));
                    if ui.add_enabled(!self.busy, egui::Button::new("Check")).clicked() { self.inspect_game(ui.ctx().clone()); }
                });
                ui.label(&self.game_status);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy, egui::Button::new("Test adapter connection")).clicked() {
                    self.test_adapter(ui.ctx().clone());
                }
                if self.diagnostic.is_some() && ui.button("Disconnect test").clicked() {
                    self.diagnostic = None;
                    self.adapter_status = "Not connected".into();
                }
            });
            ui.label(&self.adapter_status);
            ui.add_space(6.0);
            ui.label(RichText::new("This preview does not attach to the game or change items, saves, or game files.").small().weak());
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
            .with_inner_size([820.0, 590.0])
            .with_min_inner_size([680.0, 540.0])
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
