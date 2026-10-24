use anyhow::Result;
use eframe::egui;
use renamer::{engine::*, protocol, rules::*};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
enum Event {
    Refreshed(Result<Vec<Item>>),
    Items(Result<(Vec<Item>, Vec<Item>)>),
    Preview(u64, Result<Vec<Entry>>),
    SaveError(String),
    Done(Result<Vec<String>>),
    Regex(Result<Value>),
    Progress(usize, usize),
}
#[derive(Default)]
struct PreviewState {
    generation: u64,
    running: bool,
    cancel: Arc<AtomicBool>,
}
impl PreviewState {
    fn invalidate(&mut self) {
        self.generation += 1;
        self.cancel.store(true, Ordering::Relaxed);
    }
    fn start(&mut self) -> (u64, Arc<AtomicBool>) {
        self.running = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        (self.generation, self.cancel.clone())
    }
    fn finish(&mut self, generation: u64) -> bool {
        self.running = false;
        generation == self.generation
    }
}
fn current_preset(request: &Request) -> Value {
    json!({"version":1,"compound_extensions":request.compound_extensions,"rules":request.rules})
}
fn restore_preset(path: &std::path::Path, request: &mut Request) -> Result<()> {
    let value: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
    anyhow::ensure!(value["version"] == 1, "Unsupported current preset version");
    let rules: Vec<Rule> = serde_json::from_value(value["rules"].clone())?;
    let compound = value["compound_extensions"]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("Invalid compound extensions setting"))?;
    request.rules = rules;
    request.compound_extensions = compound;
    Ok(())
}
struct PresetWriter {
    tx: Option<mpsc::Sender<Value>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl PresetWriter {
    fn new(path: std::path::PathBuf, events: mpsc::Sender<Event>) -> Self {
        let (tx, rx) = mpsc::channel::<Value>();
        let worker = std::thread::spawn(move || {
            while let Ok(mut value) = rx.recv() {
                for latest in rx.try_iter() {
                    value = latest;
                }
                if let Err(error) = save_json(&path, &value) {
                    let _ = events.send(Event::SaveError(format!(
                        "Could not save current rules: {error}"
                    )));
                }
            }
        });
        Self {
            tx: Some(tx),
            worker: Some(worker),
        }
    }
    fn save(&self, value: Value) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(value);
        }
    }
}
impl Drop for PresetWriter {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
pub struct App {
    preview_state: PreviewState,
    preset_writer: PresetWriter,
    saved_preset: Value,
    engine: Engine,
    request: Request,
    items: Vec<Item>,
    excluded: HashSet<String>,
    folder_roots: Vec<Item>,
    added_roots: HashSet<String>,
    include_roots: bool,
    entries: Vec<Entry>,
    selected: Option<usize>,
    rule_index: usize,
    add_kind: String,
    message: String,
    busy: bool,
    executing: bool,
    undoing: bool,
    dirty: Option<Instant>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
    regex_text: String,
    regex_result: Option<Value>,
    example: usize,
}
pub fn launch(engine: Engine, request: Option<Request>) -> Result<()> {
    let icon = image::load_from_memory(include_bytes!("../assets/renamer-icon.png"))?.into_rgba8();
    let (w, h) = icon.dimensions();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Renamer")
            .with_inner_size([1200., 780.])
            .with_min_inner_size([900., 600.])
            .with_icon(egui::IconData {
                rgba: icon.into_raw(),
                width: w,
                height: h,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Renamer",
        options,
        Box::new(move |_| {
            let (tx, rx) = mpsc::channel();
            let preset_path = engine.root.join("current-preset.json");
            let explicit_request = request.is_some();
            let mut request = request.unwrap_or_else(|| Request {
                contents: true,
                rules: vec![Rule::new("Word format")],
                ..Default::default()
            });
            let mut load_error = None;
            if !explicit_request
                && preset_path.exists()
                && let Err(error) = restore_preset(&preset_path, &mut request)
            {
                load_error = Some(format!("Could not restore current rules: {error}"));
            }
            let mut app = App {
                preview_state: PreviewState::default(),
                preset_writer: PresetWriter::new(preset_path, tx.clone()),
                saved_preset: current_preset(&request),
                engine,
                request,
                items: vec![],
                excluded: HashSet::new(),
                folder_roots: vec![],
                added_roots: HashSet::new(),
                include_roots: false,
                entries: vec![],
                selected: None,
                rule_index: 0,
                add_kind: "Case".into(),
                message:
                    "Add files or folders to begin. Changes are applied only when you click Rename."
                        .into(),
                busy: false,
                executing: false,
                undoing: false,
                dirty: None,
                tx,
                rx,
                cancel: Arc::new(AtomicBool::new(false)),
                regex_text: "HTTPServer 2026.txt".into(),
                regex_result: None,
                example: 0,
            };
            if let Some(error) = load_error {
                app.message = error;
            } else {
                app.preset_writer.save(app.saved_preset.clone());
            }
            if !app.request.paths.is_empty() {
                app.scan();
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
impl App {
    fn changed(&mut self) {
        self.preview_state.invalidate();
        self.dirty = Some(Instant::now());
        self.entries.clear();
    }
    fn begin(&mut self) {
        self.preview_state.invalidate();
        self.busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
    }
    fn scan(&mut self) {
        self.begin();
        let r = self.request.clone();
        let tx = self.tx.clone();
        let c = self.cancel.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Items(scan_folder_selection(&r, &c)));
        });
    }
    fn toggle_roots(&mut self) {
        if self.include_roots {
            let mut seen: HashSet<String> = self.items.iter().map(|i| i.path.clone()).collect();
            for root in &self.folder_roots {
                if seen.insert(root.path.clone()) {
                    self.added_roots.insert(root.path.clone());
                    self.items.push(root.clone());
                }
            }
        } else {
            self.items.retain(|i| !self.added_roots.contains(&i.path));
            self.added_roots.clear();
        }
        self.selected = None;
        sort_items(
            &mut self.items,
            &self.request.order,
            self.request.descending,
        );
        self.changed();
    }
    fn refresh(&mut self) {
        self.begin();
        let items = self.items.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = items
                .iter()
                .map(|i| Item::read(std::path::Path::new(&i.path)))
                .collect();
            let _ = tx.send(Event::Refreshed(result));
        });
    }
    fn preview(&mut self) {
        let (generation, c) = self.preview_state.start();
        self.dirty = None;
        let items = self
            .items
            .iter()
            .filter(|i| !self.excluded.contains(&i.path))
            .cloned()
            .collect();
        let rules = self.request.rules.clone();
        let compound = self.request.compound_extensions;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Preview(
                generation,
                protocol::preview(items, rules, compound, &c),
            ));
        });
    }
    fn execute(&mut self, undo: bool) {
        self.begin();
        self.executing = true;
        self.undoing = undo;
        let engine = self.engine.clone();
        let mut entries = self.entries.clone();
        let tx = self.tx.clone();
        let c = self.cancel.clone();
        std::thread::spawn(move || {
            let progress = |a, b| {
                let _ = tx.send(Event::Progress(a, b));
            };
            let result = if undo {
                engine.undo(None, progress)
            } else {
                engine
                    .execute(&mut entries, &id(), &c, progress)
                    .map(|_| entries.into_iter().map(|e| e.final_path).collect())
            };
            let _ = tx.send(Event::Done(result));
        });
    }
    fn preset(&mut self, save: bool) {
        if save {
            if let Some(path) = rfd::FileDialog::new()
                .set_directory(self.engine.root.join("presets"))
                .set_file_name("preset.json")
                .save_file()
            {
                match save_json(
                    &path,
                    &json!({"version":1,"compound_extensions":self.request.compound_extensions,"rules":self.request.rules}),
                ) {
                    Ok(_) => self.message = "Preset saved.".into(),
                    Err(e) => self.message = e.to_string(),
                }
            }
        } else if let Some(path) = rfd::FileDialog::new()
            .set_directory(self.engine.root.join("presets"))
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            let result = (|| -> Result<Vec<Rule>> {
                let v: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
                self.request.compound_extensions =
                    v["compound_extensions"].as_bool().unwrap_or(true);
                let rules: Vec<Rule> = serde_json::from_value(v["rules"].clone())?;
                for r in &rules {
                    r.validate()?;
                }
                Ok(rules)
            })();
            match result {
                Ok(r) => {
                    self.request.rules = r;
                    self.rule_index = 0;
                    self.changed();
                }
                Err(e) => self.message = e.to_string(),
            }
        }
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Rules");
        ui.menu_button("Built-in presets", |ui| {
            for (label, data) in [
                (
                    "Clean parentheses",
                    include_str!("../presets/clean-names.json"),
                ),
                (
                    "Number photos",
                    include_str!("../presets/numbered-photos.json"),
                ),
                ("Snake case", include_str!("../presets/snake-case.json")),
            ] {
                if ui.button(label).clicked() {
                    let v: Value = serde_json::from_str(data).unwrap();
                    self.request.rules = serde_json::from_value(v["rules"].clone()).unwrap();
                    self.rule_index = 0;
                    self.changed();
                    ui.close();
                }
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("kind")
                .selected_text(&self.add_kind)
                .show_ui(ui, |ui| {
                    for k in kinds() {
                        ui.selectable_value(&mut self.add_kind, k.clone(), k);
                    }
                });
            if ui.button("Add").clicked() {
                self.request.rules.push(Rule::new(&self.add_kind));
                self.rule_index = self.request.rules.len() - 1;
                self.changed();
            }
        });
        let before = serde_json::to_string(&self.request.rules).unwrap();
        for (i, r) in self.request.rules.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut r.enabled, "");
                ui.selectable_value(&mut self.rule_index, i, format!("{}. {}", i + 1, r.kind));
            });
        }
        ui.horizontal(|ui| {
            let n = self.request.rules.len();
            if ui.button("Up").clicked() && self.rule_index > 0 {
                self.request
                    .rules
                    .swap(self.rule_index, self.rule_index - 1);
                self.rule_index -= 1;
            }
            if ui.button("Down").clicked() && self.rule_index + 1 < n {
                self.request
                    .rules
                    .swap(self.rule_index, self.rule_index + 1);
                self.rule_index += 1;
            }
            if ui.button("Duplicate").clicked() && n > 0 {
                self.request
                    .rules
                    .insert(self.rule_index, self.request.rules[self.rule_index].clone());
            }
            if ui.button("Remove").clicked() && n > 0 {
                self.request.rules.remove(self.rule_index);
                self.rule_index = self
                    .rule_index
                    .min(self.request.rules.len().saturating_sub(1));
            }
        });
        ui.separator();
        if let Some(rule) = self.request.rules.get_mut(self.rule_index) {
            ui.heading(&rule.kind);
            egui::ComboBox::from_id_salt("target")
                .selected_text(&rule.target)
                .show_ui(ui, |ui| {
                    for target in ["Name", "Extension", "Full name"] {
                        ui.selectable_value(&mut rule.target, target.into(), target);
                    }
                });
            let modes = modes(&rule.kind);
            if !modes.is_empty() {
                let mut mode = rule.s("mode", &modes[0]).to_string();
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(&mode)
                    .show_ui(ui, |ui| {
                        for m in modes {
                            ui.selectable_value(&mut mode, m.clone(), m);
                        }
                    });
                rule.options["mode"] = json!(mode);
            }
            for key in catalog()["operations"][&rule.kind]["options"]
                .as_array()
                .unwrap()
            {
                let key = key.as_str().unwrap();
                match key {
                    "mode" => {}
                    "ignore_case" | "first" | "split_numbers" | "per_folder" => {
                        let mut v = rule.b(key);
                        ui.checkbox(&mut v, key.replace('_', " "));
                        rule.options[key] = json!(v);
                    }
                    "start" | "step" | "digits" | "position" | "count" => {
                        let default = match key {
                            "start" | "step" | "count" => 1,
                            "digits" => 3,
                            _ => 0,
                        };
                        let mut v = rule.n(key, default);
                        ui.horizontal(|ui| {
                            ui.label(key);
                            ui.add(egui::DragValue::new(&mut v));
                        });
                        rule.options[key] = json!(v);
                    }
                    _ => {
                        let mut v = rule
                            .s(key, if key == "separator" { "_" } else { "" })
                            .to_string();
                        ui.label(key.replace('_', " "));
                        ui.add(egui::TextEdit::singleline(&mut v).desired_width(f32::INFINITY));
                        rule.options[key] = json!(v);
                    }
                }
            }
            if rule.kind == "Regular expression" {
                let cat = catalog();
                let examples = cat["regex_examples"].as_array().unwrap();
                egui::ComboBox::from_id_salt("examples")
                    .selected_text("Examples")
                    .show_ui(ui, |ui| {
                        for (i, e) in examples.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.example,
                                i,
                                e["title"]
                                    .as_str()
                                    .or(e["name"].as_str())
                                    .unwrap_or("Example"),
                            );
                        }
                    });
                let example = &examples[self.example];
                ui.label(example["description"].as_str().unwrap_or(""));
                ui.label(format!(
                    "{} → {}",
                    example["input"].as_str().unwrap_or(""),
                    example["output"].as_str().unwrap_or("")
                ));
                ui.horizontal(|ui| {
                    if ui.button("Use example").clicked() {
                        rule.options["pattern"] = example["pattern"].clone();
                        rule.options["replacement"] = example["replacement"].clone();
                        self.regex_text = example["input"].as_str().unwrap_or("").into();
                        self.regex_result = None;
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx()
                            .copy_text(example["pattern"].as_str().unwrap_or("").into());
                    }
                });
                ui.collapsing("Regex guide",|ui|{ui.label(r"^ start · $ end · . any character · \d digit · \s whitespace · [a-z] class · * zero or more · + one or more · ? optional · ( ) capture · \g<1> replacement group");ui.label("Rust fancy-regex supports common Python patterns and Python-style replacements. Some Python-specific expressions differ. Evaluations have a time limit.");});
                ui.label("Test text");
                ui.text_edit_singleline(&mut self.regex_text);
                if ui.button("Test expression").clicked() {
                    let options = rule.options.clone();
                    let text = self.regex_text.clone();
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(Event::Regex(protocol::worker(
                            json!({"kind":"regex","options":options,"text":text}),
                            &AtomicBool::new(false),
                        )));
                    });
                }
                if let Some(v) = &self.regex_result {
                    ui.label(format!("Result: {}", v["result"].as_str().unwrap_or("")));
                    let mut layout = egui::text::LayoutJob::default();
                    let ranges = v["matches"].as_array();
                    for (i, ch) in self.regex_text.chars().enumerate() {
                        let matched = ranges.is_some_and(|a| {
                            a.iter().any(|r| {
                                i >= r[0].as_u64().unwrap_or(0) as usize
                                    && i < (r[1].as_u64().unwrap_or(0) as usize)
                            })
                        });
                        layout.append(
                            &ch.to_string(),
                            0.,
                            egui::TextFormat {
                                color: ui.visuals().text_color(),
                                background: if matched {
                                    egui::Color32::from_rgb(80, 90, 150)
                                } else {
                                    egui::Color32::TRANSPARENT
                                },
                                ..Default::default()
                            },
                        );
                    }
                    ui.label(layout);
                }
            }
        }
        if before != serde_json::to_string(&self.request.rules).unwrap() {
            self.changed();
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Import preset").clicked() {
                self.preset(false);
            }
            if ui.button("Export preset").clicked() {
                self.preset(true);
            }
        });
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Refreshed(r) => {
                    self.busy = false;
                    match r {
                        Ok(items) => {
                            self.items = items;
                            self.changed();
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Event::Progress(a, b) => self.message = format!("Processing {a}/{b}…"),
                Event::Regex(r) => match r {
                    Ok(v) => self.regex_result = Some(v),
                    Err(e) => self.message = format!("{e:#}"),
                },
                Event::Items(r) => {
                    self.busy = false;
                    match r {
                        Ok((items, roots)) => {
                            let mut known: HashSet<String> =
                                self.folder_roots.iter().map(|i| i.path.clone()).collect();
                            self.folder_roots
                                .extend(roots.into_iter().filter(|i| known.insert(i.path.clone())));
                            let mut seen: HashSet<String> =
                                self.items.iter().map(|i| i.path.clone()).collect();
                            self.items
                                .extend(items.into_iter().filter(|i| seen.insert(i.path.clone())));
                            sort_items(
                                &mut self.items,
                                &self.request.order,
                                self.request.descending,
                            );
                            self.toggle_roots();
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Event::SaveError(error) => self.message = error,
                Event::Preview(generation, r) => {
                    if !self.preview_state.finish(generation) {
                        continue;
                    }
                    match r {
                        Ok(entries) => {
                            self.message = format!("{} items ready for review", entries.len());
                            self.entries = entries;
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Event::Done(r) => {
                    self.busy = false;
                    self.executing = false;
                    match r {
                        Ok(paths) => {
                            let journal = self.engine.read().ok().flatten();
                            for root in &mut self.folder_roots {
                                let destination = journal
                                    .as_ref()
                                    .and_then(|j| {
                                        j.entries.iter().find(|e| {
                                            if self.undoing {
                                                e.final_path == root.path
                                            } else {
                                                e.item.path == root.path
                                            }
                                        })
                                    })
                                    .map(|e| {
                                        if self.undoing {
                                            e.item.path.clone()
                                        } else {
                                            e.final_path.clone()
                                        }
                                    })
                                    .unwrap_or_else(|| root.path.clone());
                                if let Ok(updated) = Item::read(std::path::Path::new(&destination))
                                {
                                    *root = updated;
                                }
                            }
                            self.added_roots = self
                                .folder_roots
                                .iter()
                                .filter(|r| paths.contains(&r.path) && self.include_roots)
                                .map(|r| r.path.clone())
                                .collect();
                            self.items = paths
                                .iter()
                                .filter_map(|p| Item::read(std::path::Path::new(p)).ok())
                                .collect();
                            self.excluded.clear();
                            self.message = "Operation completed.".into();
                            self.changed();
                        }
                        Err(e) => {
                            self.message = format!("{e:#}");
                            self.entries.clear();
                        }
                    }
                }
            }
        }
        if !self.busy && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::R)) {
            self.refresh();
        }
        if self.executing && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.message =
                "Wait for completion or cancel and allow rollback before closing.".into();
        }
        if !self.busy
            && !self.preview_state.running
            && self
                .dirty
                .is_some_and(|t| t.elapsed() > Duration::from_millis(300))
        {
            self.preview();
        }
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.as_ref().and_then(|p| p.to_str().map(String::from)))
                .collect()
        });
        if !dropped.is_empty() && !self.busy {
            self.request.contents = true;
            self.request.paths = dropped;
            self.scan();
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_enabled_ui(!self.busy, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Add files…").clicked()
                        && let Some(paths) = rfd::FileDialog::new().pick_files()
                    {
                        self.request.contents=true;
                        self.request.paths =
                            paths.iter().map(|p| p.to_string_lossy().into()).collect();
                        self.scan();
                    }
                    if ui.button("Add folder…").clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_folder()
                    {
                        self.request.contents=true;
                        self.request.paths = vec![p.to_string_lossy().into()];
                        self.scan();
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh();
                    }
                    if ui.button("Clear").clicked() {
                        self.items.clear();
                        self.folder_roots.clear();
                        self.added_roots.clear();
                        self.selected = None;
                        self.excluded.clear();
                        self.changed();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.include_roots,"Also rename selected folders").on_hover_text("Include the folders you added, as well as their contents. Updates the current list immediately.").changed(){self.toggle_roots();}
                    ui.checkbox(&mut self.request.recursive, "Include subfolder contents");
                    ui.checkbox(&mut self.request.hidden, "Hidden");
                    egui::ComboBox::from_id_salt("filterkind")
                        .selected_text(&self.request.kind)
                        .show_ui(ui, |ui| {
                            for k in ["Both", "Files", "Folders"] {
                                ui.selectable_value(&mut self.request.kind, k.into(), k);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Name filter");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.request.name_filter)
                            .desired_width(130.),
                    );
                    ui.label("Extensions (comma separated)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.request.extensions)
                            .desired_width(130.),
                    );
                    ui.label("Filters apply when adding items");
                });
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let errors = self.entries.iter().filter(|e| !e.error.is_empty()).count();
                let changed = self
                    .entries
                    .iter()
                    .filter(|e| basename(&e.item.path) != e.name)
                    .count();
                if ui
                    .add_enabled(
                        !self.busy
                            && !self.preview_state.running
                            && self.dirty.is_none()
                            && errors == 0
                            && changed > 0,
                        egui::Button::new(format!("Rename {changed} items")),
                    )
                    .clicked()
                {
                    self.execute(false);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Undo last batch / Recover"))
                    .clicked()
                {
                    self.execute(true);
                }
                if self.busy {
                    ui.spinner();
                    if ui
                        .add_enabled(!self.undoing, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                }
                ui.label(format!("{errors} conflicts"));
            });
            ui.label(&self.message);
        });
        egui::SidePanel::left("rules")
            .resizable(true)
            .default_width(310.)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.editor(ui));
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!self.busy, |ui| {
                ui.horizontal(|ui| {
                    let old = (
                        self.request.order.clone(),
                        self.request.descending,
                        self.request.compound_extensions,
                    );
                    egui::ComboBox::from_id_salt("sort")
                        .selected_text(&self.request.order)
                        .show_ui(ui, |ui| {
                            for k in ["Name", "Path", "Modified", "Manual"] {
                                ui.selectable_value(&mut self.request.order, k.into(), k);
                            }
                        });
                    ui.checkbox(&mut self.request.descending, "Descending");
                    ui.checkbox(&mut self.request.compound_extensions, "Compound extensions");
                    if old
                        != (
                            self.request.order.clone(),
                            self.request.descending,
                            self.request.compound_extensions,
                        )
                    {
                        sort_items(
                            &mut self.items,
                            &self.request.order,
                            self.request.descending,
                        );
                        self.changed();
                    }
                    for (label, delta) in [("Move up", -1isize), ("Move down", 1)] {
                        if ui.button(label).clicked()
                            && let Some(i) = self.selected
                        {
                            let j = i as isize + delta;
                            if j >= 0 && (j as usize) < self.items.len() {
                                if i >= self.items.len() {
                                    self.selected = None;
                                    continue;
                                }
                                self.items.swap(i, j as usize);
                                self.selected = Some(j as usize);
                                self.request.order = "Manual".into();
                                self.changed();
                            }
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Use");
                ui.add_sized([210., 20.], egui::Label::new("Original name"));
                ui.add_sized([210., 20.], egui::Label::new("New name / Status"));
                ui.label("Location (hover for final path)");
            });
            let lookup: std::collections::HashMap<_, _> = self
                .entries
                .iter()
                .map(|e| (e.item.path.clone(), e.clone()))
                .collect();
            egui::ScrollArea::both().show_rows(ui, 26., self.items.len(), |ui, range| {
                for i in range {
                    let item = self.items[i].clone();
                    ui.horizontal(|ui| {
                        let mut checked = !self.excluded.contains(&item.path);
                        if ui
                            .add_enabled(!self.busy, egui::Checkbox::without_text(&mut checked))
                            .changed()
                        {
                            if checked {
                                self.excluded.remove(&item.path);
                            } else {
                                self.excluded.insert(item.path.clone());
                            }
                            self.changed();
                        }
                        if ui
                            .add_sized(
                                [210., 24.],
                                egui::Button::selectable(
                                    self.selected == Some(i),
                                    basename(&item.path),
                                ),
                            )
                            .clicked()
                        {
                            self.selected = Some(i);
                        }
                        if let Some(e) = lookup.get(&item.path) {
                            ui.add_sized(
                                [210., 24.],
                                egui::Label::new(
                                    egui::RichText::new(if e.error.is_empty() {
                                        &e.name
                                    } else {
                                        &e.error
                                    })
                                    .color(
                                        if e.error.is_empty() {
                                            ui.visuals().text_color()
                                        } else {
                                            egui::Color32::LIGHT_RED
                                        },
                                    ),
                                ),
                            )
                            .on_hover_text(&e.final_path);
                        } else {
                            ui.add_sized(
                                [210., 24.],
                                egui::Label::new(if checked {
                                    "Waiting for preview"
                                } else {
                                    "Excluded"
                                }),
                            );
                        }
                        ui.label(parent(&item.path).display().to_string());
                    });
                }
            });
        });
        let preset = current_preset(&self.request);
        if preset != self.saved_preset {
            self.preset_writer.save(preset.clone());
            self.saved_preset = preset;
        }
        if self.busy || self.preview_state.running || self.dirty.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_cancels_preview_and_rejects_stale_results() {
        let mut preview = PreviewState::default();
        let (first, cancel) = preview.start();
        preview.invalidate();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(preview.running); // Wait for cancellation before starting another worker.
        assert!(!preview.finish(first));
        let (latest, cancel) = preview.start();
        assert!(!cancel.load(Ordering::Relaxed));
        assert!(preview.finish(latest));
    }

    #[test]
    fn current_rules_survive_batch_and_restart() {
        let temp = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(temp.path().join("state"))).unwrap();
        let source = temp.path().join("HELLO.txt");
        std::fs::write(&source, "sample").unwrap();
        let request = Request {
            rules: vec![Rule {
                kind: "Case".into(),
                enabled: true,
                target: "Name".into(),
                options: json!({"mode":"lowercase"}),
            }],
            compound_extensions: false,
            ..Default::default()
        };
        let path = engine.root.join("current-preset.json");
        let (tx, _rx) = mpsc::channel();
        let writer = PresetWriter::new(path.clone(), tx);
        writer.save(current_preset(&request));
        drop(writer); // Closing the app flushes pending writes.
        let mut plan = build_plan(
            vec![Item::read(&source).unwrap()],
            request.rules.clone(),
            false,
        )
        .unwrap();
        engine
            .execute(
                &mut plan,
                "remember-rules",
                &AtomicBool::new(false),
                |_, _| {},
            )
            .unwrap();
        assert!(temp.path().join("hello.txt").exists());
        let mut restored = Request::default();
        restore_preset(&path, &mut restored).unwrap();
        assert_eq!(current_preset(&restored), current_preset(&request));
        assert!(restored.paths.is_empty());
        // Empty rule lists are intentional and must not be replaced by defaults.
        restored.rules.clear();
        save_json(&path, &current_preset(&restored)).unwrap();
        restore_preset(&path, &mut restored).unwrap();
        assert!(restored.rules.is_empty());
    }

    #[test]
    fn writer_keeps_latest_edit_and_preserves_incomplete_regex() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("current-preset.json");
        let (tx, _rx) = mpsc::channel();
        let writer = PresetWriter::new(path.clone(), tx);
        let mut request = Request::default();
        for pattern in ["a", "ab", "["] {
            request.rules = vec![Rule {
                kind: "Regular expression".into(),
                enabled: true,
                target: "Name".into(),
                options: json!({"pattern":pattern,"replacement":""}),
            }];
            writer.save(current_preset(&request));
        }
        drop(writer);
        let mut restored = Request::default();
        restore_preset(&path, &mut restored).unwrap();
        assert_eq!(current_preset(&restored), current_preset(&request));
    }
}
