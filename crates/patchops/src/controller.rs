//! The state/controller layer: the single object every view talks to.
//!
//! One `Entity<Controller>` is created at startup and handed to every view. It owns
//!
//! * the `Arc<Core>` and the latest `Arc<State>` snapshot (views read it in `render`; cloning the
//!   `Arc` is free, nothing big is copied per frame),
//! * per-action busy flags (the legacy `busy` ids; see [`Action::busy_key`]),
//! * the error strip text, the toast host and the Activity Log list,
//! * the global dialogs (confirm, depot prompt, directory browser) which the shell renders,
//! * the navigation selection.
//!
//! Mutating actions run strictly one at a time on the background executor (some ops take
//! seconds); the independent, non-mutating ones ([`Action::is_independent`]: open URL, launch,
//! copy logs, refresh) start immediately so they never wait behind a download. Results are
//! applied on the UI thread; a state older than the newest one already applied is dropped (every
//! [`Reply`](crate::actions::Reply) carries a sequence stamp). There is no polling: the state is
//! re-read after each action (the op returns it), on the Refresh button, and when the in-GUI bench
//! RPC mutated something. The only timer that can exist is the 5 s depot watch while the depot
//! prompt is open; its mutating swap goes through the same queue.
//!
//! See `GPUI_NOTES.md` section "Writing a view" for the conventions.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AppContext, AsyncApp, ClipboardItem, Context, Entity, EventEmitter, SharedString, Subscription, Task,
    WeakEntity,
};
use patchops_core::log::LogEntry;
use patchops_core::ops::basics;
use patchops_core::{Core, State};

use crate::actions::{exec, next_seq, Action, ReplyData};
use crate::app::Nav;
use crate::state::{Boot, UiEvent};
use crate::widgets::{BrowseData, BrowseEntry, BrowseLocation, DirBrowser, DirBrowserEvent, LogList, ToastHost, ToastKind};

/// Legacy `backendUnavailableMessage("starting")`.
const STARTING_MESSAGE: &str = "PatchOpsIII is still getting ready.";
/// Ring size of the core log; the Activity Log keeps the same number of lines.
const LOG_LINES: usize = 300;

/// What the directory browser is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseTarget {
    /// Pick the game directory; confirming runs `Action::SetGameDirectory`.
    GameDir,
    /// Pick a folder for the Enhanced dump source; confirming emits
    /// [`ControllerEvent::DirSelected`] (the Enhanced view subscribes).
    DumpFolder,
}

/// A "Are you sure?" prompt that runs `action` when confirmed.
#[derive(Clone, Debug)]
pub struct Confirm {
    pub title: SharedString,
    pub message: SharedString,
    pub confirm_label: SharedString,
    pub destructive: bool,
    pub action: Action,
}

impl Confirm {
    pub fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        action: Action,
    ) -> Self {
        Self { title: title.into(), message: message.into(), confirm_label: confirm_label.into(), destructive: false, action }
    }

    /// Red confirm button.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// The prompt the legacy UI (`window.confirm`) showed before this action, if any.
    pub fn legacy(action: &Action) -> Option<Confirm> {
        let (title, message, label) = match action {
            Action::T7Uninstall => ("Uninstall T7 Patch?", "Uninstall T7 Patch files and restore LPC backups?", "Uninstall"),
            Action::EnhancedUninstall => {
                ("Uninstall BO3 Enhanced?", "Uninstall BO3 Enhanced files and restore backups?", "Uninstall")
            }
            Action::DxvkUninstall => {
                ("Uninstall DXVK-GPLAsync?", "Uninstall DXVK-GPLAsync and remove dxvk.conf?", "Uninstall")
            }
            Action::ClearModFiles => ("Clear mod files?", "Clear cached downloaded mod files?", "Clear"),
            Action::ResetStock => {
                ("Reset to stock?", "Reset PatchOpsIII-managed files and settings to stock?", "Reset to Stock")
            }
            _ => return None,
        };
        Some(Confirm::new(title, message, label, action.clone()).destructive())
    }
}

/// The "Copy this" dialog of the compatible-EXE flow (legacy `DepotPromptState`).
#[derive(Clone, Debug)]
pub struct DepotPrompt {
    pub command: SharedString,
    pub copied: bool,
    pub watching: bool,
}

/// Completion notice of one [`Action`], sent to subscribers of the controller.
#[derive(Clone, Debug)]
pub struct Finished {
    /// [`Action::busy_key`] of the action.
    pub key: &'static str,
    pub ok: bool,
    pub data: ReplyData,
}

#[derive(Clone, Debug)]
pub enum ControllerEvent {
    /// An action finished (success or failure). The state has already been applied.
    Finished(Finished),
    /// The directory browser confirmed a path for a target that is handled by the caller
    /// (`BrowseTarget::DumpFolder`).
    DirSelected { target: BrowseTarget, path: String },
}

struct Browser {
    view: Entity<DirBrowser>,
    _sub: Subscription,
}

pub struct Controller {
    core: Arc<Core>,
    boot: Boot,
    state: Option<Arc<State>>,
    /// Stamp of the newest applied state (see `apply_state`).
    state_seq: u64,
    nav: Nav,
    /// Keys of queued + running actions (a key may appear more than once).
    busy: Vec<&'static str>,
    queue: VecDeque<Action>,
    running: bool,
    refreshing: bool,
    refresh_again: bool,
    /// Download percent of the running network install `(busy key, percent)`.
    progress: Option<(&'static str, u8)>,
    error: Option<SharedString>,
    /// "Older install found" strip text (`None` = nothing found or dismissed).
    legacy_notice: Option<SharedString>,
    toasts: Entity<ToastHost>,
    logs: Entity<LogList>,
    last_error_log: Option<SharedString>,
    log_feed: async_channel::Receiver<LogEntry>,
    confirm: Option<Confirm>,
    depot: Option<DepotPrompt>,
    depot_task: Option<Task<()>>,
    browser: Option<Browser>,
    /// The next directory-browser navigation is the dialog's initial one (a missing start folder
    /// silently falls back to `$HOME` instead of showing an error).
    browse_initial: bool,
    _tasks: Vec<Task<()>>,
}

impl EventEmitter<ControllerEvent> for Controller {}

impl Controller {
    pub fn new(
        core: Arc<Core>,
        ui_rx: async_channel::Receiver<UiEvent>,
        log_rx: async_channel::Receiver<LogEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Exact hand-off of the early log: snapshot the ring first, then drain what the
        // subscription already buffered; entries in both are listed once, none are lost.
        let ring = core.log.recent(LOG_LINES);
        let mut buffered = Vec::new();
        while let Ok(e) = log_rx.try_recv() {
            buffered.push(e);
        }
        let early = merge_early_logs(ring, buffered);
        let toasts = cx.new(|_| ToastHost::new());
        let logs = cx.new(|cx| LogList::new(cx).max_lines(LOG_LINES));
        let mut tasks = Vec::new();

        // Core log -> Activity Log. The logging thread only does a channel send; this task drains
        // whatever has accumulated and appends it with a single repaint.
        let feed = log_rx.clone();
        tasks.push(cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Ok(first) = feed.recv().await {
                let mut batch = vec![first];
                while batch.len() < 512 {
                    match feed.try_recv() {
                        Ok(e) => batch.push(e),
                        Err(_) => break,
                    }
                }
                if this.update(cx, |this, cx| this.push_logs(batch, cx)).is_err() {
                    break;
                }
            }
        }));

        // In-GUI bench RPC changed things behind our back: re-read the state (coalesced).
        tasks.push(cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Ok(UiEvent::Refresh) = ui_rx.recv().await {
                while ui_rx.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.refresh_silent(cx)).is_err() {
                    break;
                }
            }
        }));

        let mut this = Self {
            core: core.clone(),
            boot: Boot::Starting,
            state: None,
            state_seq: 0,
            nav: Nav::Dashboard,
            busy: Vec::new(),
            queue: VecDeque::new(),
            running: false,
            refreshing: false,
            refresh_again: false,
            progress: None,
            error: None,
            legacy_notice: None,
            toasts,
            logs,
            last_error_log: None,
            log_feed: log_rx,
            confirm: None,
            depot: None,
            depot_task: None,
            browser: None,
            browse_initial: false,
            _tasks: tasks,
        };
        // Entries logged before the UI existed (settings migration, ...).
        this.push_logs(early, cx);
        this.boot(cx);
        this.detect_legacy(cx);
        this.trim_log(cx);
        this
    }

    /// Start-up bound of the app log (a 2 MiB read plus writes and fsyncs when it is over the cap;
    /// a single stat otherwise), off the main thread so it cannot delay the first frame. This is the
    /// only process kind that trims at start-up; appends enforce the cap afterwards.
    fn trim_log(&mut self, cx: &mut Context<Self>) {
        let core = self.core.clone();
        cx.background_executor().spawn(async move { core.log.trim_if_needed() }).detach();
    }

    // ---- reading -------------------------------------------------------------------------

    /// Latest state; `None` only before the first load finished (the shell shows the startup
    /// screen then, so views can normally `let Some(s) = c.state() else { return empty }`).
    pub fn state(&self) -> Option<&Arc<State>> {
        self.state.as_ref()
    }

    pub fn boot_status(&self) -> &Boot {
        &self.boot
    }

    pub fn ready(&self) -> bool {
        self.boot == Boot::Ready
    }

    /// Is an action with this busy key queued or running? Use for `.busy(..)` / `.disabled(..)`.
    pub fn is_busy(&self, key: &'static str) -> bool {
        self.busy.contains(&key)
    }

    /// Download percent (0..=100) of the running network action with this busy key
    /// (`t7-install`, `dxvk-install`, `enhanced-install`); `None` when idle or the size is unknown
    /// (show an indeterminate bar while `is_busy`).
    pub fn progress(&self, key: &'static str) -> Option<u8> {
        self.progress.filter(|(k, _)| *k == key).map(|(_, p)| p)
    }

    pub fn any_busy(&self) -> bool {
        !self.busy.is_empty()
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn legacy_notice(&self) -> Option<&SharedString> {
        self.legacy_notice.as_ref()
    }

    /// Look for a leftover old Windows install off the UI thread (registry read; `None` elsewhere).
    fn detect_legacy(&mut self, cx: &mut Context<Self>) {
        let core = self.core.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let found = cx.background_executor().spawn(async move { crate::state::detect_legacy_notice(&core) }).await;
            if let Some(text) = found {
                this.update(cx, |this, cx| {
                    this.legacy_notice = Some(text.into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Hide the strip and remember that for next time (a marker file; never waits on the operation lock).
    pub fn dismiss_legacy_notice(&mut self, cx: &mut Context<Self>) {
        self.legacy_notice = None;
        cx.notify();
        let core = self.core.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = crate::state::dismiss_legacy_notice(&core);
            })
            .detach();
    }

    pub fn nav(&self) -> Nav {
        self.nav
    }

    pub fn logs(&self) -> &Entity<LogList> {
        &self.logs
    }

    pub fn toasts(&self) -> &Entity<ToastHost> {
        &self.toasts
    }

    /// Number of lines in the Activity Log.
    pub fn log_count(&self, cx: &gpui::App) -> usize {
        self.logs.read(cx).len()
    }

    /// Message of the newest Error log entry seen since the last clear (Tools "Last error").
    pub fn last_error_log(&self) -> Option<&SharedString> {
        self.last_error_log.as_ref()
    }

    pub fn confirm_dialog(&self) -> Option<&Confirm> {
        self.confirm.as_ref()
    }

    pub fn depot_prompt(&self) -> Option<&DepotPrompt> {
        self.depot.as_ref()
    }

    pub fn browser(&self) -> Option<&Entity<DirBrowser>> {
        self.browser.as_ref().map(|b| &b.view)
    }

    // ---- acting --------------------------------------------------------------------------

    /// Queue `action`. Marks its busy key, clears the error strip, runs it on the background
    /// executor and applies the returned state / error. One line per button:
    /// `controller.update(cx, |c, cx| c.run(Action::IntroSkip(true), cx))`.
    pub fn run(&mut self, action: Action, cx: &mut Context<Self>) {
        if !self.ready() {
            self.error = Some(STARTING_MESSAGE.into());
            cx.notify();
            return;
        }
        self.error = None;
        // Busy from the moment of the click (queued counts): the triggering control shows its
        // spinner / disabled state immediately, not only once the op starts.
        self.busy.push(action.busy_key());
        if action.is_independent() {
            self.spawn_exec(action, false, cx);
        } else {
            self.queue.push_back(action);
            self.pump(cx);
        }
        cx.notify();
    }

    /// Mutating actions waiting behind the running one.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Like [`run`](Self::run), but first asks the legacy confirmation if the action had one
    /// (uninstalls, clear mod files, reset to stock).
    pub fn run_confirmed(&mut self, action: Action, cx: &mut Context<Self>) {
        match Confirm::legacy(&action) {
            Some(c) => self.confirm(c, cx),
            None => self.run(action, cx),
        }
    }

    /// Show a confirm dialog; the action runs only if the user confirms.
    pub fn confirm(&mut self, c: Confirm, cx: &mut Context<Self>) {
        self.confirm = Some(c);
        cx.notify();
    }

    pub(crate) fn resolve_confirm(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            if yes {
                self.run(c.action, cx);
            }
        }
        cx.notify();
    }

    pub fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        cx.notify();
    }

    pub fn toast(&mut self, kind: ToastKind, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        let msg = msg.into();
        self.toasts.update(cx, |t, cx| t.push(kind, msg, cx));
    }

    /// Open `https://...` / `steam://...` with the system handler (core's `open_external_url`).
    pub fn open_url(&mut self, url: impl Into<String>, cx: &mut Context<Self>) {
        self.run(Action::OpenUrl(url.into()), cx);
    }

    pub fn navigate(&mut self, nav: Nav, cx: &mut Context<Self>) {
        if self.nav != nav {
            self.nav = nav;
            cx.notify();
        }
    }

    /// Open the directory browser modal. `start` = initial folder (`None` = the saved game
    /// directory, then `$HOME`).
    pub fn open_browser(&mut self, target: BrowseTarget, start: Option<String>, cx: &mut Context<Self>) {
        let (title, desc, label, need_exe) = match target {
            BrowseTarget::GameDir => (
                "Select Game Directory",
                "Choose the folder that contains BlackOps3.exe or BlackOpsIII.exe.",
                "Use Selected Folder",
                true,
            ),
            BrowseTarget::DumpFolder => (
                "Select Dump Folder",
                "Choose an extracted dump folder, or type a DUMP.zip path above.",
                "Use Dump Folder",
                false,
            ),
        };
        let view = cx.new(|cx| DirBrowser::new(title, desc, label, cx).require_game_executable(need_exe));
        let sub = cx.subscribe(&view, move |this, view, ev: &DirBrowserEvent, cx| match ev {
            DirBrowserEvent::Navigate(p) => this.browse_to(p.clone(), view.downgrade(), cx),
            DirBrowserEvent::Select(p) => {
                this.browser = None;
                match target {
                    BrowseTarget::GameDir => this.run(Action::SetGameDirectory(p.clone()), cx),
                    BrowseTarget::DumpFolder => cx.emit(ControllerEvent::DirSelected { target, path: p.clone() }),
                }
                cx.notify();
            }
            DirBrowserEvent::Cancel => {
                this.browser = None;
                cx.notify();
            }
        });
        let start = start
            .filter(|s| !s.is_empty())
            .or_else(|| self.state.as_ref().and_then(|s| s.game_dir.clone()))
            .unwrap_or_default();
        self.browser = Some(Browser { view: view.clone(), _sub: sub });
        self.browse_initial = true;
        view.update(cx, |b, cx| b.navigate(start, cx));
        cx.notify();
    }

    /// Re-read the state without busy flags or error handling (RPC wake-ups, boot).
    pub fn refresh_silent(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            self.refresh_again = true;
            return;
        }
        self.refreshing = true;
        let core = self.core.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let s = cx
                .background_executor()
                .spawn(async move {
                    let s = basics::status(&core);
                    (s, next_seq())
                })
                .await;
            this.update(cx, |this, cx| {
                this.refreshing = false;
                this.apply_state(s.0, s.1);
                cx.notify();
                if std::mem::take(&mut this.refresh_again) {
                    this.refresh_silent(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Retry after a failed startup (legacy "Retry" button).
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.boot(cx);
    }

    // ---- depot prompt --------------------------------------------------------------------

    pub fn depot_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.depot.as_mut() {
            let cmd = d.command.to_string();
            d.copied = true;
            cx.write_to_clipboard(ClipboardItem::new_string(cmd));
            cx.notify();
        }
    }

    /// "Continue": open the Steam console and start watching for the depot.
    pub fn depot_continue(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.depot.as_mut() else { return };
        if d.watching {
            return;
        }
        d.watching = true;
        self.open_url("steam://open/console", cx);
        self.start_depot_watch(cx);
        cx.notify();
    }

    pub fn depot_cancel(&mut self, cx: &mut Context<Self>) {
        self.depot = None;
        self.depot_task = None;
        cx.notify();
    }

    /// The loop in `start_depot_watch` notices `watching == false` and exits by itself.
    fn stop_depot_watch(&mut self) {
        if let Some(d) = self.depot.as_mut() {
            d.watching = false;
        }
    }

    /// Legacy `pollCompatibleDepot` every 5 s while the prompt is watching: when the depot is
    /// there, queue the compatible swap (the prompt closes when it succeeds, see `finish`). The
    /// only timer in the app. A tick is skipped while anything is queued or running, and the
    /// swap itself goes through the serial queue, so it can never run concurrently with another
    /// exe operation or Reset to Stock on the same files.
    fn start_depot_watch(&mut self, cx: &mut Context<Self>) {
        let core = self.core.clone();
        self.depot_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                let Ok(idle) = this.update(cx, |t, _| match t.depot.as_ref().map(|d| d.watching) {
                    Some(true) => Some(!t.running && t.queue.is_empty()),
                    _ => None,
                }) else {
                    break;
                };
                match idle {
                    None => break, // prompt closed or no longer watching
                    Some(true) => {
                        let c = core.clone();
                        // The depot check only reads; it may overlap with the UI.
                        let check = cx
                            .background_executor()
                            .spawn(async move { exec(&c, &Action::ExeDepotCheck, &mut |_| {}) })
                            .await;
                        let applied = this.update(cx, |t, cx| {
                            if t.depot.as_ref().map(|d| d.watching) != Some(true) {
                                return;
                            }
                            let available = check.ok && matches!(check.data, ReplyData::DepotAvailable(true));
                            if let Some(s) = check.state {
                                t.apply_state(s, check.seq);
                            }
                            // Something may have been queued while the check ran.
                            if available && !t.running && t.queue.is_empty() {
                                t.run(Action::ExeCompatible, cx);
                            }
                            cx.notify();
                        });
                        if applied.is_err() {
                            break;
                        }
                    }
                    Some(false) => {}
                }
                cx.background_executor().timer(Duration::from_secs(5)).await;
            }
        }));
    }

    // ---- internals -----------------------------------------------------------------------

    fn boot(&mut self, cx: &mut Context<Self>) {
        self.boot = Boot::Starting;
        cx.notify();
        let core = self.core.clone();
        // Dev aids for looking at the startup screens (debug builds only):
        // PATCHOPS_BOOT_DELAY_MS=3000, PATCHOPS_BOOT_FAIL=1.
        #[cfg(debug_assertions)]
        let delay = std::env::var("PATCHOPS_BOOT_DELAY_MS").ok().and_then(|v| v.parse::<u64>().ok());
        #[cfg(debug_assertions)]
        let fail = std::env::var_os("PATCHOPS_BOOT_FAIL").is_some_and(|v| v == "1");
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            #[cfg(debug_assertions)]
            if let Some(ms) = delay {
                cx.background_executor().timer(Duration::from_millis(ms)).await;
            }
            // `basics::status` cannot fail; the Failed screen is only reachable through the
            // debug-only PATCHOPS_BOOT_FAIL switch.
            let res: Result<(State, u64), ()> = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(debug_assertions)]
                    if fail {
                        return Err(());
                    }
                    let s = basics::status(&core);
                    Ok((s, next_seq()))
                })
                .await;
            this.update(cx, |this, cx| {
                match res {
                    Ok((s, seq)) => {
                        this.apply_state(s, seq);
                        this.boot = Boot::Ready;
                    }
                    Err(()) => {
                        this.boot = Boot::Failed("PatchOpsIII could not read its state. Restart PatchOpsIII and try again.".into())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Keep the newest snapshot: a state stamped older than the one already applied (a slow
    /// refresh finishing after a later op) is dropped.
    fn apply_state(&mut self, s: State, seq: u64) {
        if seq >= self.state_seq {
            self.state_seq = seq;
            self.state = Some(Arc::new(s));
        }
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let Some(action) = self.queue.pop_front() else { return };
        self.running = true;
        self.progress = None;
        self.spawn_exec(action, true, cx);
    }

    /// Run `action` on the background executor. `serial` = it holds the queue slot (`running`),
    /// and the next queued action starts when it completes.
    fn spawn_exec(&mut self, action: Action, serial: bool, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let a = action.clone();
        let key = action.busy_key();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            // Download percent of the network installers; the worker only does a channel send per
            // percent change. The forwarding task is dropped (cancelled) when the op returns.
            let (ptx, prx) = async_channel::unbounded::<u8>();
            let weak = this.clone();
            let forward = cx.spawn(async move |cx: &mut AsyncApp| {
                while let Ok(p) = prx.recv().await {
                    if weak
                        .update(cx, |t, cx| {
                            t.progress = Some((key, p));
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let reply = cx
                .background_executor()
                .spawn(async move {
                    let mut last = u8::MAX;
                    exec(&core, &a, &mut |p| {
                        if p != last {
                            last = p;
                            let _ = ptx.try_send(p);
                        }
                    })
                })
                .await;
            drop(forward);
            this.update(cx, |this, cx| {
                if serial {
                    this.progress = None;
                    this.running = false;
                }
                this.finish(action, reply, cx);
                if serial {
                    this.pump(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, action: Action, reply: crate::actions::Reply, cx: &mut Context<Self>) {
        let key = action.busy_key();
        if let Some(i) = self.busy.iter().position(|k| *k == key) {
            self.busy.remove(i);
        }
        if let Some(s) = reply.state {
            if matches!(action, Action::ClearLogs) {
                // The ring was emptied by the op; rebuild the list from what is left.
                while self.log_feed.try_recv().is_ok() {}
                self.last_error_log = None;
                let entries = s.logs.clone();
                self.logs.update(cx, |l, cx| l.clear(cx));
                self.push_logs(entries, cx);
            }
            self.apply_state(s, reply.seq);
        }
        if !reply.ok {
            match (&action, &reply.data) {
                (Action::ExeCompatible, ReplyData::DepotRequired { command }) => match self.depot.as_mut() {
                    // Still not downloaded while the prompt watches: keep prompt and watch as they are.
                    Some(d) => d.command = command.clone().into(),
                    None => {
                        self.depot = Some(DepotPrompt { command: command.clone().into(), copied: false, watching: false })
                    }
                },
                _ => {
                    if matches!(action, Action::ExeCompatible) {
                        self.stop_depot_watch();
                    }
                    let msg: SharedString = reply.error.clone().unwrap_or_else(|| "Action failed".into()).into();
                    self.error = Some(msg);
                }
            }
        } else if matches!(action, Action::ExeCompatible) && self.depot.take().is_some() {
            // The depot watch (or the prompt's own Continue) finished the swap.
            self.depot_task = None;
            self.nav = Nav::ExeSwapper;
        } else if let (Action::CopyLogs, ReplyData::Payload(p)) = (&action, &reply.data) {
            cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
            self.toast(ToastKind::Success, "Logs copied to clipboard", cx);
        }
        cx.emit(ControllerEvent::Finished(Finished { key, ok: reply.ok, data: reply.data }));
        cx.notify();
    }

    fn push_logs(&mut self, batch: Vec<LogEntry>, cx: &mut Context<Self>) {
        if batch.is_empty() {
            return;
        }
        if let Some(e) = batch.iter().rev().find(|e| e.category == "Error") {
            self.last_error_log = Some(e.message.clone().into());
        }
        self.logs.update(cx, |l, cx| {
            l.extend(batch.iter().map(|e| (e.category.as_str(), SharedString::from(e.message.clone()))), cx)
        });
        cx.notify();
    }

    fn browse_to(&mut self, path: String, view: WeakEntity<DirBrowser>, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let initial = std::mem::take(&mut self.browse_initial);
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = cx.background_executor().spawn(async move { browse_listing(&core, &path, initial) }).await;
            view.update(cx, |b, cx| match result {
                Ok(data) => b.set_data(data, cx),
                Err(msg) => b.set_error(msg, cx),
            })
            .ok();
        })
        .detach();
    }
}

/// List `path` through core's `browse` op. Core silently falls back to `$HOME` for a missing path,
/// which is right for the dialog's initial folder but wrong for a path the user typed or clicked:
/// those are checked here first so the dialog can say so instead of jumping to the home folder.
fn browse_listing(core: &Core, path: &str, initial: bool) -> Result<BrowseData, String> {
    if !path.is_empty() && !initial {
        let dir = patchops_core::pyfmt::expanduser(path, &core.paths.home);
        if !dir.is_dir() {
            return Err(format!("Folder not found: {path}"));
        }
        if let Err(e) = std::fs::read_dir(&dir) {
            return Err(format!("Cannot open {path}: {e}"));
        }
    }
    let payload = basics::browse(core, (!path.is_empty()).then_some(path));
    let loc = |r: Vec<basics::Root>| r.into_iter().map(|r| BrowseLocation { label: r.label, path: r.path }).collect::<Vec<_>>();
    Ok(BrowseData {
        path: payload.path,
        parent: payload.parent,
        has_game_executable: payload.has_game_executable,
        roots: loc(payload.roots),
        shortcuts: loc(payload.shortcuts),
        entries: payload
            .entries
            .into_iter()
            .map(|e| BrowseEntry { name: e.name, path: e.path, has_game_executable: e.has_game_executable })
            .collect(),
    })
}

/// Combine the core log ring (snapshot taken first) with the entries the log subscription
/// buffered meanwhile. The newest ring entries are normally also in the buffer: they are listed
/// once; everything older in the ring (logged before the subscription) is kept.
fn merge_early_logs(ring: Vec<LogEntry>, buffered: Vec<LogEntry>) -> Vec<LogEntry> {
    let keep = (0..=ring.len())
        .find(|&i| {
            let tail = &ring[i..];
            tail.is_empty() || buffered.windows(tail.len()).any(|w| w == tail)
        })
        .unwrap_or(ring.len());
    let mut out = ring;
    out.truncate(keep);
    out.extend(buffered);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(m: &str) -> LogEntry {
        LogEntry { message: m.into(), category: "Info".into(), line: format!("t - Info: {m}") }
    }
    fn msgs(v: &[LogEntry]) -> Vec<&str> {
        v.iter().map(|e| e.message.as_str()).collect()
    }

    #[test]
    fn overlap_is_listed_once() {
        // ring = a b | c d (c d also buffered), then e arrived after the snapshot.
        let out = merge_early_logs(vec![e("a"), e("b"), e("c"), e("d")], vec![e("c"), e("d"), e("e")]);
        assert_eq!(msgs(&out), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn nothing_buffered_keeps_the_ring() {
        assert_eq!(msgs(&merge_early_logs(vec![e("a"), e("b")], vec![])), ["a", "b"]);
    }

    #[test]
    fn everything_buffered_is_not_duplicated() {
        assert_eq!(msgs(&merge_early_logs(vec![e("a"), e("b")], vec![e("a"), e("b")])), ["a", "b"]);
    }

    #[test]
    fn no_overlap_concatenates() {
        assert_eq!(msgs(&merge_early_logs(vec![e("a")], vec![e("b")])), ["a", "b"]);
    }

    #[test]
    fn empty_ring_returns_buffer() {
        assert_eq!(msgs(&merge_early_logs(vec![], vec![e("b")])), ["b"]);
    }

    #[test]
    fn ring_wrapped_inside_the_buffered_run() {
        // The ring only holds the tail of what the subscription buffered.
        let out = merge_early_logs(vec![e("c"), e("d")], vec![e("a"), e("b"), e("c"), e("d"), e("e")]);
        assert_eq!(msgs(&out), ["a", "b", "c", "d", "e"]);
    }
}
