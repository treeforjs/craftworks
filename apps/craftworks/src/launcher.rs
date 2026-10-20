use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use craft_manager::{APPS, App, Availability, Installed, Manager};
use craft_ui::{Appearance, Palette, PaletteItem, Theme, Variant};
use craft_update::{AppInfo, Error, GitHub, Release, Source, State, Updater, Version};
use egui::{RichText, Stroke, vec2};

const RELEASES: &str = "https://github.com/treeforjs/craftworks/releases";

struct AppView {
    app: App,
    installed: Option<Installed>,
    installed_error: Option<String>,
    available: Option<Availability>,
    check_error: Option<String>,
    action_error: Option<String>,
    child: Option<Child>,
    launching: bool,
}

impl AppView {
    fn new(app: App) -> Self {
        Self { app, installed: None, installed_error: None, available: None, check_error: None, action_error: None, child: None, launching: false }
    }

    fn running(&self) -> bool {
        self.child.is_some() || self.launching
    }

    fn newer_release(&self) -> Option<&Release> {
        let available = self.available.as_ref()?;
        if available.blocked.is_some() || available.asset.is_none() || self.running() {
            return None;
        }
        let release = available.release.as_ref()?;
        if let Some(installed) = &self.installed {
            let current = Version::parse(&installed.version)?;
            if release.version <= current {
                return None;
            }
        }
        Some(release)
    }
}

struct Active {
    app: App,
    label: String,
    received: u64,
    total: u64,
    cancel: Arc<AtomicBool>,
    cancellable: bool,
}

type InstalledResult = Result<Option<Installed>, Error>;
type Initialization = Result<(Manager, Vec<(App, InstalledResult)>, Vec<String>), Error>;

enum Message {
    Initialized(Initialization),
    Checked(App, Result<Availability, Error>),
    CheckFinished,
    Progress(App, u64, u64),
    Installed(App, Result<Installed, Error>),
    Removed(App, Result<(), Error>),
    Launched(App, Result<Child, Error>),
    Failed(Job, String),
}

#[derive(Clone, Copy)]
enum Job {
    Initialize,
    Check,
    Mutation(App),
    Launch(App),
}

enum Action {
    Check,
    Install(App),
    InstallAll,
    UpdateAll,
    Open(App),
    Remove(App),
    Cancel,
}

/// The desktop app. Filesystem and network work run on workers; rendering stays responsive.
pub struct Launcher {
    theme: Theme,
    palette: Palette,
    settings_open: bool,
    apps: Vec<AppView>,
    manager: Option<Manager>,
    initializing: bool,
    checking: bool,
    checked_at: Option<Instant>,
    fatal_error: Option<String>,
    notices: Vec<String>,
    active: Option<Active>,
    queue: VecDeque<(App, Release)>,
    confirm_remove: Option<App>,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    updater: Option<Updater>,
    last_child_poll: Instant,
    close_after_cancel: bool,
}

impl Launcher {
    /// Restore appearance and start the installed-app scan without waiting for disk or network.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        craft_ui::fonts::install(&cc.egui_ctx, egui::FontDefinitions::default());
        let mut app = Self::empty();
        if let Some(storage) = cc.storage {
            app.theme = eframe::get_value(storage, "craftworks-theme").unwrap_or_default();
        }
        app.initialize(&cc.egui_ctx);
        app
    }

    fn empty() -> Self {
        let (tx, rx) = channel();
        Self {
            theme: Theme::default(),
            palette: Palette::default(),
            settings_open: false,
            apps: APPS.into_iter().map(AppView::new).collect(),
            manager: None,
            initializing: false,
            checking: false,
            checked_at: None,
            fatal_error: None,
            notices: Vec::new(),
            active: None,
            queue: VecDeque::new(),
            confirm_remove: None,
            tx,
            rx,
            updater: None,
            last_child_poll: Instant::now(),
            close_after_cancel: false,
        }
    }

    fn initialize(&mut self, ctx: &egui::Context) {
        self.initializing = true;
        self.fatal_error = None;
        let source: Arc<dyn Source> = Arc::new(GitHub::new(format!("Craftworks/{}", env!("CARGO_PKG_VERSION"))));
        let mut updater = Updater::new(
            AppInfo {
                id: "craftworks".into(),
                display_name: "Craftworks".into(),
                repo: "treeforjs/craftworks".into(),
                current: env!("CARGO_PKG_VERSION").into(),
                public_key: select_key(option_env!("CRAFTWORKS_UPDATE_PUBLIC_KEY"), option_env!("CRAFT_UPDATE_PUBLIC_KEY")).map(str::to_owned),
                prereleases: false,
            },
            source.clone(),
        );
        let repaint = ctx.clone();
        updater.on_change(move || repaint.request_repaint());
        self.updater = Some(updater);
        self.spawn(ctx, Job::Initialize, move |tx| {
            let result = (|| {
                let mut manager = Manager::new(Manager::default_root()?, source)?;
                let mut notices = Vec::new();
                for app in APPS {
                    if let Some(key) = configured_key(app.id) {
                        match manager.clone().with_public_key(app.id, key) {
                            Ok(configured) => manager = configured,
                            Err(error) => notices.push(format!("{} downloads are unavailable: {error}", app.name)),
                        }
                    }
                }
                let installed = APPS.into_iter().map(|app| (app, manager.installed(app))).collect();
                Ok((manager, installed, notices))
            })();
            let _ = tx.send(Message::Initialized(result));
        });
    }

    fn spawn(&self, ctx: &egui::Context, kind: Job, work: impl FnOnce(Sender<Message>) + Send + 'static) {
        let tx = self.tx.clone();
        let failure_tx = self.tx.clone();
        let repaint = ctx.clone();
        let result = std::thread::Builder::new().name("craftworks-worker".into()).spawn(move || {
            if catch_unwind(AssertUnwindSafe(|| work(tx))).is_err() {
                let _ = failure_tx.send(Message::Failed(kind, "The background task stopped unexpectedly. Please try again.".into()));
            }
            repaint.request_repaint();
        });
        if let Err(error) = result {
            let _ = self.tx.send(Message::Failed(kind, format!("Couldn't start the background task: {error}")));
            ctx.request_repaint();
        }
    }

    fn view_mut(&mut self, app: App) -> Option<&mut AppView> {
        self.apps.iter_mut().find(|view| view.app.id == app.id)
    }

    fn check(&mut self, ctx: &egui::Context) {
        if self.checking || self.initializing {
            return;
        }
        let Some(manager) = self.manager.clone() else {
            self.initialize(ctx);
            return;
        };
        self.checking = true;
        if let Some(updater) = &mut self.updater {
            updater.check();
        }
        let repaint = ctx.clone();
        self.spawn(ctx, Job::Check, move |tx| {
            for app in APPS {
                let _ = tx.send(Message::Checked(app, manager.check(app)));
                repaint.request_repaint();
            }
            let _ = tx.send(Message::CheckFinished);
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Initialized(result) => {
                    self.initializing = false;
                    match result {
                        Ok((manager, installed, notices)) => {
                            self.manager = Some(manager);
                            self.notices = notices;
                            for (app, result) in installed {
                                if let Some(view) = self.view_mut(app) {
                                    match result {
                                        Ok(installed) => view.installed = installed,
                                        Err(error) => view.installed_error = Some(error.to_string()),
                                    }
                                }
                            }
                            self.check(ctx);
                        }
                        Err(error) => self.fatal_error = Some(error.to_string()),
                    }
                }
                Message::Checked(app, result) => {
                    if let Some(view) = self.view_mut(app) {
                        match result {
                            Ok(available) => {
                                view.available = Some(available);
                                view.check_error = None;
                            }
                            Err(error) => view.check_error = Some(format!("Couldn't check for updates: {error}. Try Check for updates again.")),
                        }
                    }
                }
                Message::CheckFinished => {
                    self.checking = false;
                    self.checked_at = Some(Instant::now());
                }
                Message::Progress(app, received, total) => {
                    if let Some(active) = &mut self.active
                        && active.app.id == app.id
                    {
                        active.received = received;
                        active.total = total;
                    }
                }
                Message::Installed(app, result) => {
                    self.active = None;
                    if let Some(view) = self.view_mut(app) {
                        match result {
                            Ok(installed) => {
                                view.installed = Some(installed);
                                view.installed_error = None;
                                view.action_error = None;
                            }
                            Err(Error::Cancelled) => {
                                view.action_error = Some(if view.installed.is_some() {
                                    "Installation cancelled. The previous installed version is still available.".into()
                                } else {
                                    "Installation cancelled. You can try again at any time.".into()
                                })
                            }
                            Err(error) => view.action_error = Some(format!("Couldn't install {}: {error}. Try again or visit Releases.", app.name)),
                        }
                    }
                }
                Message::Removed(app, result) => {
                    self.active = None;
                    if let Some(view) = self.view_mut(app) {
                        match result {
                            Ok(()) => {
                                view.installed = None;
                                view.installed_error = None;
                                view.action_error = None;
                            }
                            Err(error) => view.action_error = Some(format!("Couldn't remove {}: {error}", app.name)),
                        }
                    }
                }
                Message::Launched(app, result) => {
                    if let Some(view) = self.view_mut(app) {
                        view.launching = false;
                        match result {
                            Ok(child) => {
                                view.child = Some(child);
                                view.action_error = None;
                            }
                            Err(error) => view.action_error = Some(format!("Couldn't open {}: {error}", app.name)),
                        }
                    }
                }
                Message::Failed(job, message) => match job {
                    Job::Initialize => {
                        self.initializing = false;
                        self.fatal_error = Some(message);
                    }
                    Job::Check => {
                        self.checking = false;
                        self.notices.push(message);
                    }
                    Job::Mutation(app) => {
                        self.active = None;
                        self.queue.clear();
                        if let Some(view) = self.view_mut(app) {
                            view.action_error = Some(message);
                        }
                    }
                    Job::Launch(app) => {
                        if let Some(view) = self.view_mut(app) {
                            view.launching = false;
                            view.action_error = Some(message);
                        }
                    }
                },
            }
        }
        if let Some(updater) = &mut self.updater {
            updater.poll();
        }
        if self.last_child_poll.elapsed() >= Duration::from_millis(500) {
            self.last_child_poll = Instant::now();
            for view in &mut self.apps {
                if let Some(child) = &mut view.child {
                    match child.try_wait() {
                        Ok(None) => {}
                        Ok(Some(status)) => {
                            view.child = None;
                            if !status.success() {
                                view.action_error = Some(format!(
                                    "{} closed with an error ({status}). Check the app's release notes for runtime requirements.",
                                    view.app.name
                                ));
                            }
                        }
                        Err(error) => {
                            // Keep the handle and removal disabled when the OS cannot establish
                            // that the process has exited.
                            view.action_error = Some(format!("Couldn't check whether {} has closed: {error}", view.app.name));
                        }
                    }
                }
            }
        }
        if self.active.is_none() {
            self.start_next(ctx);
            if self.close_after_cancel && self.active.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.initializing || self.checking || self.active.is_some() || self.apps.iter().any(AppView::running) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn start_next(&mut self, ctx: &egui::Context) {
        if self.active.is_some() {
            return;
        }
        let Some(manager) = self.manager.clone() else { return };
        let Some((app, release)) = self.queue.pop_front() else { return };
        if self.apps.iter().any(|view| view.app.id == app.id && view.running()) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.active = Some(Active {
            app,
            label: format!("Installing {} {}", app.name, release.version),
            received: 0,
            total: 0,
            cancel: cancel.clone(),
            cancellable: true,
        });
        let repaint = ctx.clone();
        self.spawn(ctx, Job::Mutation(app), move |tx| {
            let result = manager.install_with_cancel(
                app,
                &release,
                &mut |received, total| {
                    let _ = tx.send(Message::Progress(app, received, total));
                    repaint.request_repaint();
                },
                &cancel,
            );
            let _ = tx.send(Message::Installed(app, result));
        });
    }

    fn dispatch(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Check => self.check(ctx),
            Action::Install(app) => {
                if self.active.is_none()
                    && self.queue.is_empty()
                    && let Some(release) = self.apps.iter().find(|view| view.app.id == app.id).and_then(AppView::newer_release).cloned()
                {
                    self.queue.push_back((app, release));
                }
            }
            Action::InstallAll | Action::UpdateAll => {
                if self.active.is_none() && self.queue.is_empty() {
                    let update = matches!(action, Action::UpdateAll);
                    for view in &self.apps {
                        if view.installed.is_some() == update
                            && let Some(release) = view.newer_release()
                        {
                            self.queue.push_back((view.app, release.clone()));
                        }
                    }
                }
            }
            Action::Open(app) => {
                let blocked = self.active.is_some() || !self.queue.is_empty();
                let Some(manager) = self.manager.clone().filter(|_| !blocked) else { return };
                if let Some(view) = self.view_mut(app) {
                    if view.running() || view.installed.is_none() {
                        return;
                    }
                    view.launching = true;
                }
                self.spawn(ctx, Job::Launch(app), move |tx| {
                    let _ = tx.send(Message::Launched(app, manager.launch(app)));
                });
            }
            Action::Remove(app) => {
                if self.active.is_none() && self.queue.is_empty() && self.apps.iter().any(|view| view.app.id == app.id && !view.running()) {
                    self.confirm_remove = Some(app);
                    self.settings_open = false;
                    self.palette.open = false;
                }
            }
            Action::Cancel => {
                self.queue.clear();
                if let Some(active) = &self.active
                    && active.cancellable
                {
                    active.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        self.start_next(ctx);
    }

    fn remove(&mut self, ctx: &egui::Context, app: App) {
        let Some(manager) = self.manager.clone() else { return };
        if self.active.is_some() || self.apps.iter().any(|view| view.app.id == app.id && view.running()) {
            return;
        }
        self.active =
            Some(Active { app, label: format!("Removing {}", app.name), received: 0, total: 0, cancel: Arc::default(), cancellable: false });
        self.spawn(ctx, Job::Mutation(app), move |tx| {
            let _ = tx.send(Message::Removed(app, manager.uninstall(app)));
        });
    }

    /// Draw a deterministic, network-free preview for screenshots and embedding tests.
    pub fn preview(appearance: Appearance) -> Self {
        let mut app = Self::empty();
        app.theme.appearance = appearance;
        for view in &mut app.apps {
            view.available = Some(Availability { release: None, asset: None, blocked: Some("No published release yet".into()) });
        }
        app.checked_at = Some(Instant::now());
        app
    }

    /// A network-free fixture demonstrating that an offline check preserves installed apps.
    pub fn preview_offline(appearance: Appearance) -> Self {
        let mut app = Self::empty();
        app.theme.appearance = appearance;
        app.notices.push("Preview fixture · The update server is unavailable.".into());
        for view in &mut app.apps {
            view.installed = Some(Installed { version: "0.9.0".into(), executable: std::path::PathBuf::from(view.app.id) });
            view.check_error = Some("Couldn't check for updates: the update server is unavailable. Installed apps are ready to open.".into());
        }
        app.checked_at = Some(Instant::now());
        app
    }

    /// A network-free fixture showing a release whose downloads cannot be verified by this build.
    pub fn preview_without_key(appearance: Appearance) -> Self {
        let mut app = Self::empty();
        app.theme.appearance = appearance;
        app.notices.push("Preview fixture · Published releases, with no verification key configured.".into());
        for view in &mut app.apps {
            if let Some(version) = Version::parse("1.2.0") {
                view.available = Some(Availability {
                    release: Some(Release {
                        version,
                        tag: "v1.2.0".into(),
                        page: format!("https://github.com/{}/releases/tag/v1.2.0", view.app.repo),
                        notes: "Example release notes. New features, fixes and improvements.".into(),
                        assets: Vec::new(),
                    }),
                    asset: None,
                    blocked: Some("This build has no verification key for this app. Visit Releases for available downloads.".into()),
                });
            }
        }
        app.checked_at = Some(Instant::now());
        app
    }

    /// Draw the launcher; also used by the headless gallery.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let t = craft_ui::apply(&ctx, &self.theme);
        let mut action = None;
        let busy = self.initializing || self.active.is_some() || !self.queue.is_empty();
        if ctx.input_mut(|input| input.consume_shortcut(&craft_ui::shortcuts::PALETTE)) {
            self.palette.open();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma))) {
            self.settings_open = true;
        }
        egui::Frame::new().fill(t.bg).inner_margin(egui::Margin::same(24)).show(ui, |ui| {
            ui.set_width(ui.available_width().max(0.0));
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
                ui.set_width(viewport.width().max(0.0));
                let narrow = ui.available_width() < 520.0;
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Craftworks").size(26.0));
                    ui.label(RichText::new(env!("CARGO_PKG_VERSION")).small().color(t.text_muted));
                    if !narrow {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.header_actions(ui));
                    }
                });
                if narrow {
                    ui.horizontal(|ui| self.header_actions(ui));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Your Craft apps, together.").size(18.0));
                ui.add(egui::Label::new(RichText::new("Install, keep up to date, and get back to creating.").color(t.text_muted)).wrap());
                ui.add_space(20.0);
                let update_count = self.apps.iter().filter(|view| view.installed.is_some() && view.newer_release().is_some()).count();
                let install_count = self.apps.iter().filter(|view| view.installed.is_none() && view.newer_release().is_some()).count();
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(!self.checking && !self.initializing, |ui| {
                        if craft_ui::button(ui, if self.checking { "Checking…" } else { "Check for updates" }, Variant::Primary).clicked() {
                            action = Some(Action::Check);
                        }
                    });
                    if narrow {
                        ui.end_row();
                    }
                    ui.add_enabled_ui(!busy && update_count > 0, |ui| {
                        if craft_ui::button(ui, &format!("Update all ({update_count})"), Variant::Secondary).clicked() {
                            action = Some(Action::UpdateAll);
                        }
                    });
                    ui.add_enabled_ui(!busy && install_count > 0, |ui| {
                        if craft_ui::button(ui, &format!("Install all ({install_count})"), Variant::Secondary).clicked() {
                            action = Some(Action::InstallAll);
                        }
                    });
                });
                ui.add_space(8.0);
                let status = if self.initializing {
                    "Finding your installed apps…"
                } else if self.checking {
                    "Checking the latest published releases…"
                } else if self.apps.iter().any(|view| view.check_error.is_some()) {
                    "Couldn't check some releases. Installed apps still work offline."
                } else if self.checked_at.is_some() {
                    "Release check complete. Installed apps also work offline."
                } else {
                    "Ready to check for published releases."
                };
                ui.add(egui::Label::new(RichText::new(status).small().color(t.text_muted)).wrap());
                if let Some(error) = &self.fatal_error {
                    ui.add_space(8.0);
                    ui.colored_label(t.danger, format!("Couldn't open the app library: {error}. Use Check for updates to retry."));
                }
                for notice in &self.notices {
                    ui.colored_label(t.warning, notice);
                }
                if let Some(active) = &self.active {
                    ui.add_space(12.0);
                    egui::Frame::new().fill(t.surface).stroke(Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(12).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label(&active.label);
                            if active.cancellable && craft_ui::button(ui, "Cancel", Variant::Secondary).clicked() {
                                action = Some(Action::Cancel);
                            }
                        });
                        if active.total > 0 {
                            let fraction = (active.received as f64 / active.total as f64).clamp(0.0, 1.0) as f32;
                            ui.add(egui::ProgressBar::new(fraction).text(format!("{} / {}", bytes(active.received), bytes(active.total))));
                        } else {
                            ui.label(RichText::new("Downloading, verifying, and installing…").small().color(t.text_muted));
                        }
                        if !self.queue.is_empty() {
                            ui.label(RichText::new(format!("{} more app(s) queued", self.queue.len())).small().color(t.text_muted));
                        }
                        if active.cancel.load(Ordering::Relaxed) {
                            ui.label("Cancelling safely…");
                        }
                    });
                }
                ui.add_space(20.0);
                for view in &self.apps {
                    let working = self.active.as_ref().is_some_and(|active| active.app.id == view.app.id);
                    card(ui, view, &t, busy, working, self.initializing, self.checking, &mut action);
                    ui.add_space(12.0);
                }
                if self.apps.is_empty() {
                    craft_ui::empty_state(
                        ui,
                        &craft_ui::EmptyState {
                            icon: None,
                            title: "Your apps belong here",
                            body: "Check for updates to find the Craft apps.",
                            action: None,
                        },
                    );
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Downloads are checked against signed checksums before installation.").small().color(t.text_muted));
                if let Some(manager) = &self.manager {
                    ui.label(RichText::new(format!("App location · {}", manager.root().display())).small().color(t.text_muted));
                }
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to("Craftworks releases", RELEASES);
                    if let Some(updater) = &self.updater {
                        match updater.state() {
                            State::Available(offer) | State::Ready(offer) => {
                                ui.label(format!("Craftworks {} is available.", offer.release.version));
                                ui.hyperlink_to("Get the new launcher", &offer.release.page);
                            }
                            State::Failed(_) => {
                                ui.label(RichText::new("Launcher update check unavailable.").small().color(t.text_muted));
                            }
                            _ => {}
                        }
                    }
                });
            });
        });
        if let Some(action) = action {
            self.dispatch(&ctx, action);
        }
        self.settings(&ctx);
        self.removal_dialog(&ctx);
        self.command_palette(&ctx, busy);
    }

    fn header_actions(&mut self, ui: &mut egui::Ui) {
        if craft_ui::button(ui, "Settings", Variant::Ghost).on_hover_text("Appearance and display scale · Ctrl+,").clicked() {
            self.settings_open = true;
        }
        if craft_ui::button(ui, "Commands", Variant::Ghost).on_hover_text("Search commands · Ctrl+K").clicked() {
            self.palette.open();
        }
    }

    fn settings(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(true).default_width(640.0).vscroll(true).show(ctx, |ui| {
            if ui.available_width() >= 560.0 {
                craft_ui::appearance_settings(ui, &mut self.theme);
            } else {
                appearance(ui, &mut self.theme);
                ui.checkbox(&mut self.theme.reduced_motion, "Reduce motion");
                ui.checkbox(&mut self.theme.prefer_dark, "Prefer dark when system appearance is unavailable");
                let mut compact = self.theme.density == craft_ui::Density::Compact;
                if ui.checkbox(&mut compact, "Compact controls").changed() {
                    self.theme.density = if compact { craft_ui::Density::Compact } else { craft_ui::Density::Comfortable };
                }
            }
            ui.add_space(16.0);
            let mut scale = ctx.zoom_factor();
            if ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).text("Display scale").suffix("×").step_by(0.05)).changed() {
                ctx.set_zoom_factor(scale);
            }
            ui.add_space(12.0);
            ui.label("Keyboard · Tab moves focus, Enter activates, Ctrl+K searches commands, Ctrl+, opens settings.");
        });
        self.settings_open = open;
    }

    fn removal_dialog(&mut self, ctx: &egui::Context) {
        let Some(app) = self.confirm_remove else { return };
        let mut remove = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("remove-app")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.heading(format!("Remove {}?", app.name));
            ui.label("This removes the copy installed by Craftworks. Your documents and app settings stay in place. You can install it again later.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                remove = craft_ui::button(ui, "Remove app", Variant::Danger).clicked();
                cancel = craft_ui::button(ui, "Keep app", Variant::Secondary).clicked();
            });
        });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if remove || cancel {
            self.confirm_remove = None;
        }
        if remove {
            self.remove(ctx, app);
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context, busy: bool) {
        if self.confirm_remove.is_some() {
            return;
        }
        let mut items = vec![
            PaletteItem::command("check", "Check for updates").enabled(!self.initializing && !self.checking),
            PaletteItem::command("settings", "Appearance and display scale").shortcut("Ctrl+,"),
        ];
        for view in &self.apps {
            items.push(
                PaletteItem::command(format!("open.{}", view.app.id), format!("Open {}", view.app.name))
                    .enabled(view.installed.is_some() && !view.running() && !busy),
            );
            items.push(
                PaletteItem::command(format!("install.{}", view.app.id), format!("Install or update {}", view.app.name))
                    .enabled(!busy && view.newer_release().is_some()),
            );
        }
        if let Some(id) = self.palette.show(ctx, &items) {
            match id.as_str() {
                "check" => self.check(ctx),
                "settings" => self.settings_open = true,
                _ => {
                    for app in APPS {
                        if id == format!("open.{}", app.id) {
                            self.dispatch(ctx, Action::Open(app));
                        } else if id == format!("install.{}", app.id) {
                            self.dispatch(ctx, Action::Install(app));
                        }
                    }
                }
            }
        }
    }
}

impl eframe::App for Launcher {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|input| input.viewport().close_requested()) && self.active.is_some() {
            // Downloads can cancel; removal needs to finish. Keep the window alive until
            // the worker has stopped so no partially written installation is left behind.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_cancel = true;
            self.dispatch(ctx, Action::Cancel);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "craftworks-theme", &self.theme);
    }
}

fn configured_key(app_id: &str) -> Option<&'static str> {
    let app_key = match app_id {
        "pdfcraft" => option_env!("PDFCRAFT_UPDATE_PUBLIC_KEY"),
        "vectorcraft" => option_env!("VECTORCRAFT_UPDATE_PUBLIC_KEY"),
        "photocraft" => option_env!("PHOTOCRAFT_UPDATE_PUBLIC_KEY"),
        _ => None,
    };
    select_key(app_key, option_env!("CRAFT_UPDATE_PUBLIC_KEY"))
}

fn select_key<'a>(app_key: Option<&'a str>, shared_key: Option<&'a str>) -> Option<&'a str> {
    app_key.map(str::trim).filter(|key| !key.is_empty()).or_else(|| shared_key.map(str::trim).filter(|key| !key.is_empty()))
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 { format!("{:.1} MB", value as f64 / (1024.0 * 1024.0)) } else { format!("{:.0} KB", value as f64 / 1024.0) }
}

fn appearance(ui: &mut egui::Ui, theme: &mut Theme) {
    egui::ComboBox::from_label("Appearance").selected_text(theme.appearance.label()).show_ui(ui, |ui| {
        for appearance in Appearance::ALL {
            ui.selectable_value(&mut theme.appearance, appearance, appearance.label());
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    view: &AppView,
    t: &craft_ui::Tokens,
    busy: bool,
    working: bool,
    initializing: bool,
    checking: bool,
    action: &mut Option<Action>,
) {
    egui::Frame::new().fill(t.surface).stroke(Stroke::new(1.0, t.border)).corner_radius(10).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width().max(0.0));
        let text_width = (ui.available_width() - 44.0 - 8.0 - ui.spacing().item_spacing.x).max(0.0);
        let narrow = ui.available_width() < 400.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 10.0, t.accent_soft);
            let monogram = match view.app.id {
                "pdfcraft" => "Pdf",
                "vectorcraft" => "Vc",
                "photocraft" => "Pc",
                _ => "C",
            };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, monogram, craft_ui::fonts::semibold(ui.ctx(), 17.0), t.accent_text);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(text_width);
                ui.add(egui::Label::new(RichText::new(view.app.name).font(craft_ui::fonts::semibold(ui.ctx(), 18.0))).wrap());
                ui.add(egui::Label::new(RichText::new(view.app.summary).color(t.text_muted)).wrap());
            });
        });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            let installed = view.installed.as_ref().map_or_else(
                || if initializing { "Finding installed version…".into() } else { "Not installed".into() },
                |installed| format!("Installed {}", installed.version),
            );
            ui.label(installed);
            if narrow {
                ui.end_row();
            } else {
                ui.label(RichText::new("·").color(t.text_muted));
            }
            let latest = match &view.available {
                Some(available) => {
                    available.release.as_ref().map_or_else(|| "No published release yet".into(), |release| format!("Latest {}", release.version))
                }
                None if checking => "Checking latest…".into(),
                None => "Latest version not checked".into(),
            };
            ui.label(RichText::new(latest).color(t.text_muted));
            if view.running() {
                ui.colored_label(t.success, "Running");
            }
        });
        if view.running() {
            ui.label(RichText::new("Close the app before updating or removing it.").small().color(t.warning));
        }
        if let Some(available) = &view.available {
            if let Some(blocked) = &available.blocked {
                if available.release.is_some() {
                    ui.add(egui::Label::new(RichText::new(blocked).small().color(t.warning)).wrap());
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new("This app will be available here when its first signed release is published.").small().color(t.text_muted),
                        )
                        .wrap(),
                    );
                }
            } else if view.installed.is_some() && view.newer_release().is_none() && !view.running() {
                ui.label(RichText::new("Up to date").small().color(t.success));
            }
        }
        for error in [&view.installed_error, &view.check_error, &view.action_error].into_iter().flatten() {
            ui.add(egui::Label::new(RichText::new(error).small().color(t.danger)).wrap());
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            let label = if working {
                "Working…"
            } else if view.installed.is_some() {
                "Update"
            } else {
                "Install"
            };
            ui.add_enabled_ui(!busy && view.newer_release().is_some(), |ui| {
                if craft_ui::button(ui, label, Variant::Secondary).clicked() {
                    *action = Some(Action::Install(view.app));
                }
            });
            ui.add_enabled_ui(view.installed.is_some() && !view.running() && !busy, |ui| {
                if craft_ui::button(ui, "Open", Variant::Secondary).clicked() {
                    *action = Some(Action::Open(view.app));
                }
            });
            if narrow {
                ui.end_row();
            }
            ui.add_enabled_ui((view.installed.is_some() || view.installed_error.is_some()) && !view.running() && !busy, |ui| {
                if craft_ui::button(ui, "Remove", Variant::Ghost).clicked() {
                    *action = Some(Action::Remove(view.app));
                }
            });
            ui.hyperlink_to("Releases", format!("https://github.com/{}/releases", view.app.repo));
        });
        if let Some(release) = view.available.as_ref().and_then(|available| available.release.as_ref())
            && !release.notes.trim().is_empty()
        {
            egui::CollapsingHeader::new("Release notes").id_salt(view.app.id).show(ui, |ui| {
                ui.label(&release.notes);
                ui.hyperlink_to("Read on GitHub", &release.page);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_draws_in_every_mode_and_size() {
        for appearance in Appearance::ALL {
            for density in craft_ui::Density::ALL {
                for size in [vec2(920.0, 900.0), vec2(360.0, 480.0), vec2(0.0, 0.0)] {
                    let ctx = egui::Context::default();
                    let mut app = Launcher::preview(appearance);
                    app.theme.density = density;
                    let mut output = ctx.run_ui(
                        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() },
                        |ui| app.draw(ui),
                    );
                    output.textures_delta.clear();
                }
            }
        }
    }

    #[test]
    fn launcher_draws_an_empty_or_unavailable_library() {
        let ctx = egui::Context::default();
        let mut app = Launcher::empty();
        app.apps.clear();
        app.fatal_error = Some("The app folder is unavailable".into());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw(ui));
        output.textures_delta.clear();
    }

    #[test]
    fn missing_release_never_enables_an_install() {
        let app = Launcher::preview(Appearance::Light);
        assert!(app.apps.iter().all(|view| view.newer_release().is_none()));
    }

    #[test]
    fn empty_app_keys_fall_back_to_the_shared_key() {
        assert_eq!(select_key(Some(""), Some("shared")), Some("shared"));
        assert_eq!(select_key(Some(" \t\n"), Some(" shared ")), Some("shared"));
        assert_eq!(select_key(Some(" app "), Some("shared")), Some("app"));
        assert_eq!(select_key(None, Some("shared")), Some("shared"));
        assert_eq!(select_key(Some(""), Some("  ")), None);
        assert_eq!(select_key(None, None), None);
    }

    #[test]
    fn an_offline_check_keeps_the_installed_app() -> Result<(), Error> {
        let ctx = egui::Context::default();
        let mut app = Launcher::preview_offline(Appearance::Light);
        let catalog_app = APPS.first().copied().ok_or_else(|| Error::Invalid("empty test catalog".into()))?;
        let _ = app.tx.send(Message::Checked(catalog_app, Err(Error::Network("offline".into()))));
        app.poll(&ctx);
        let view = app.apps.first().ok_or_else(|| Error::Invalid("empty test library".into()))?;
        assert_eq!(view.installed.as_ref().map(|installed| installed.version.as_str()), Some("0.9.0"));
        assert!(view.check_error.is_some());
        assert!(!view.running());
        Ok(())
    }

    #[test]
    fn only_a_newer_installable_release_enables_update() -> Result<(), Error> {
        let mut app = Launcher::preview_without_key(Appearance::Light);
        let view = app.apps.first_mut().ok_or_else(|| Error::Invalid("empty test library".into()))?;
        assert!(view.newer_release().is_none());
        let available = view.available.as_mut().ok_or_else(|| Error::Invalid("missing fixture release".into()))?;
        available.asset =
            Some(craft_update::Asset { name: "fixture.AppImage".into(), url: "https://example.invalid/fixture.AppImage".into(), size: 1 });
        // Having a package alone never bypasses the verification-key restriction.
        assert!(view.newer_release().is_none());
        if let Some(available) = &mut view.available {
            available.blocked = None;
        }
        assert!(view.newer_release().is_some());
        view.installed = Some(Installed { version: "1.2.0".into(), executable: "fixture".into() });
        assert!(view.newer_release().is_none());
        if let Some(installed) = &mut view.installed {
            installed.version = "1.3.0".into();
        }
        assert!(view.newer_release().is_none());
        if let Some(installed) = &mut view.installed {
            installed.version = "1.0.0".into();
        }
        assert!(view.newer_release().is_some());
        view.launching = true;
        assert!(view.newer_release().is_none());
        Ok(())
    }

    #[test]
    fn settings_and_remove_confirmation_draw_at_small_sizes() {
        for size in [vec2(360.0, 480.0), vec2(0.0, 0.0)] {
            let ctx = egui::Context::default();
            let mut app = Launcher::preview(Appearance::Dark);
            app.settings_open = true;
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            let mut output = ctx.run_ui(input.clone(), |ui| app.draw(ui));
            output.textures_delta.clear();
            app.settings_open = false;
            app.confirm_remove = APPS.first().copied();
            let mut output = ctx.run_ui(input, |ui| app.draw(ui));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn compact_library_fits_the_available_width() {
        for size in [vec2(360.0, 640.0), vec2(480.0, 640.0), vec2(920.0, 880.0)] {
            for mut app in
                [Launcher::preview(Appearance::Light), Launcher::preview_offline(Appearance::Dark), Launcher::preview_without_key(Appearance::Light)]
            {
                let ctx = egui::Context::default();
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
                let mut output = ctx.run_ui(input, |ui| {
                    let right = ui.max_rect().right();
                    app.draw(ui);
                    assert!(ui.min_rect().right() <= right + 1.0, "library overflowed its available width: {:?}", ui.min_rect());
                });
                output.textures_delta.clear();
            }
        }
    }
}
