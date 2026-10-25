//! Native window operations. Preferences and fallback rules come from the application.
use super::*;

impl App {
    pub(super) fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        self.view()
    }

    pub(super) fn tray_available(&self) -> bool {
        self.tray
            .as_ref()
            .is_some_and(|tray| tray.online.load(Ordering::SeqCst) && !tray.handle.is_closed())
    }

    pub(super) fn show_window(&mut self) -> Task<Message> {
        // Ignore minimized-state replies queued before an explicit restore.
        self.window_revision = self.window_revision.wrapping_add(1);
        if let Some(id) = self.window {
            return window::minimize(id, false)
                .chain(window::gain_focus(id))
                .chain(self.check_minimized(id));
        }
        let (id, open) = window::open(window::Settings {
            size: iced::Size::new(if self.compact { 860.0 } else { 1120.0 }, 760.0),
            min_size: Some(iced::Size::new(820.0, 620.0)),
            position: window::Position::Centered,
            exit_on_close_request: false,
            icon: Some(
                window::icon::from_rgba(
                    application::APP_ICON_RGBA.to_vec(),
                    application::APP_ICON_SIZE,
                    application::APP_ICON_SIZE,
                )
                .expect("embedded TSM icon has valid RGBA dimensions"),
            ),
            platform_specific: window::settings::PlatformSpecific {
                application_id: application::APP_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        // Reserve the ID before opening so repeated tray activation cannot create duplicates.
        self.window = Some(id);
        let revision = self.window_revision;
        open.then(move |id| {
            window::is_minimized(id).map(move |state| Message::WindowMinimized(id, revision, state))
        })
    }

    pub(super) fn check_minimized(&self, id: window::Id) -> Task<Message> {
        let revision = self.window_revision;
        window::is_minimized(id).map(move |state| Message::WindowMinimized(id, revision, state))
    }

    pub(super) fn window_minimized(
        &mut self,
        id: window::Id,
        revision: u64,
        state: Option<bool>,
    ) -> Task<Message> {
        if self.window != Some(id) || revision != self.window_revision {
            return Task::none();
        }
        // Query the actual window: environment variables may also be set for XWayland.
        self.minimize_supported = Some(state.is_some());
        if self
            .settings
            .window
            .hide_minimized(state, self.tray_available())
        {
            return self.minimize_to_tray();
        }
        Task::none()
    }

    pub(super) fn close_requested(&mut self, id: window::Id) -> Task<Message> {
        if self.window != Some(id) {
            return Task::none();
        }
        match self.settings.window.on_close(self.tray_available()) {
            application::CloseAction::HideToTray => self.minimize_to_tray(),
            application::CloseAction::Quit => self.request_quit(),
        }
    }

    pub(super) fn minimize_to_tray(&mut self) -> Task<Message> {
        if !self.tray_available() {
            self.status = msg!(TrayUnavailable);
            self.error = true;
            return Task::none();
        }
        // Winit cannot hide a Wayland window. A daemon can close its surface and reopen
        // it from the tray, keeping application state and the sync coordinator alive.
        self.window_revision = self.window_revision.wrapping_add(1);
        self.window.take().map(window::close).unwrap_or_default()
    }

    pub(super) fn request_quit(&mut self) -> Task<Message> {
        self.quitting = true;
        self.busy = true;
        self.status = msg!(ClosingApp);
        self.error = false;
        match self.coordinator.clone() {
            Some(coordinator) => Task::perform(async move { coordinator.shutdown().await }, |_| {
                Message::ShutdownReady
            }),
            None => iced::exit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_start_waits_for_tray_and_failed_tray_opens_once() {
        let (mut app, _) = App::new(Args::default());
        app.window = None;
        app.settings.window.start_in_tray = true;
        app.tray_starting = true;
        let _ = app.update(Message::Tick);
        assert!(app.window.is_none(), "registration is still pending");
        let _ = app.update(Message::Tray(Err(msg!(TrayUnavailable))));
        let restored = app.window.expect("failed registration opens the window");
        assert!(!app.tray_starting);
        let _ = app.update(Message::Tick);
        assert_eq!(app.window, Some(restored));

        app.window = None;
        app.tray_starting = true;
        let _ = app.update(Message::Show);
        let requested = app.window;
        let _ = app.update(Message::Tray(Err(msg!(TrayUnavailable))));
        assert_eq!(app.window, requested, "late registration cannot undo Open");
    }

    #[test]
    fn close_without_a_tray_quits_and_old_window_requests_are_ignored() {
        for close_to_tray in [false, true] {
            let (mut app, _) = App::new(Args::default());
            app.settings.window.close_to_tray = close_to_tray;
            let id = app.window.unwrap();
            let _ = app.update(Message::WindowCloseRequested(window::Id::unique()));
            assert!(!app.quitting);
            let _ = app.update(Message::WindowCloseRequested(id));
            assert!(app.quitting);
            assert_eq!(app.window, Some(id), "shutdown waits for coordinated work");
        }
    }

    #[test]
    fn minimized_capability_comes_from_current_window_and_restore_discards_old_replies() {
        let (mut app, _) = App::new(Args::default());
        let id = app.window.unwrap();
        let old_revision = app.window_revision;
        let _ = app.update(Message::WindowMinimized(id, old_revision, None));
        assert_eq!(app.minimize_supported, Some(false));
        let _ = app.update(Message::Toggle(Flag::MinimizeToTray, true));
        assert!(!app.preferences.window.minimize_to_tray);
        let _ = app.update(Message::Show);
        let revision = app.window_revision;
        let _ = app.update(Message::WindowMinimized(id, old_revision, Some(true)));
        assert_eq!(app.minimize_supported, Some(false));
        let _ = app.update(Message::WindowMinimized(
            window::Id::unique(),
            revision,
            Some(true),
        ));
        assert_eq!(app.minimize_supported, Some(false));
        let _ = app.update(Message::WindowMinimized(id, revision, Some(false)));
        assert_eq!(app.minimize_supported, Some(true));
        let _ = app.update(Message::Toggle(Flag::MinimizeToTray, true));
        assert!(app.preferences.window.minimize_to_tray);
        assert!(
            !app.settings.window.minimize_to_tray,
            "unsaved drafts are not active"
        );
        app.settings.window.minimize_to_tray = true;
        let _ = app.update(Message::WindowMinimized(id, revision, Some(true)));
        assert_eq!(
            app.window,
            Some(id),
            "minimizing without a tray keeps the window"
        );
    }

    #[test]
    fn only_saved_preferences_control_close_and_minimize() {
        let (mut app, _) = App::new(Args::default());
        app.busy = false;
        app.settings.window = Default::default();
        app.preferences.window = Default::default();
        let saved = app.settings.clone();
        let _ = app.update(Message::Toggle(Flag::CloseToTray, false));
        assert!(app.settings.window.close_to_tray, "drafts are not active");
        let _ = app.update(Message::Finished(Outcome {
            snapshot: Snapshot::default(),
            settings: saved.clone(),
            revision: 0,
            result: Err(msg!(InvalidBackupPeriod)),
        }));
        assert!(app.settings.window.close_to_tray);
        assert!(
            app.preferences.window.close_to_tray,
            "a failed save shows the saved value again"
        );
        let mut stored = saved;
        stored.window.close_to_tray = false;
        let _ = app.update(Message::Finished(Outcome {
            snapshot: Snapshot::default(),
            settings: stored,
            revision: 1,
            result: Ok(msg!(SettingsSaved)),
        }));
        assert!(!app.settings.window.close_to_tray);
    }

    #[test]
    fn closing_stops_new_ui_work_and_does_not_minimize() {
        for busy in [false, true] {
            let (mut app, _) = App::new(Args::default());
            app.busy = busy;
            let id = app.window.expect("startup opens a window");
            let _ = app.update(Message::Quit);
            assert!(app.quitting);
            assert_eq!(app.window, Some(id));
            assert_eq!(
                app.status.render(Language::English),
                Language::English.text(Key::ClosingApp)
            );
            for action in [
                Message::Tick,
                Message::Sync,
                Message::WindowCloseRequested(id),
                Message::Show,
                Message::Quit,
            ] {
                assert_eq!(app.update(action).units(), 0);
            }
            assert_eq!(app.window, Some(id));
        }
    }

    #[test]
    fn missing_tray_cannot_hide_window_and_lost_tray_restores_it() {
        let (mut app, _) = App::new(Args::default());
        let original = app.window;
        let _ = app.minimize_to_tray();
        assert_eq!(app.window, original);
        assert!(!app.quitting);
        assert!(app.error);
        assert_eq!(
            app.status.render(Language::English),
            Language::English.text(Key::TrayUnavailable)
        );

        app.window = None;
        app.tray_starting = false;
        let _ = app.update(Message::Tick);
        let reopened = app.window.expect("a lost tray restores the window");
        let _ = app.update(Message::Show);
        assert_eq!(
            app.window,
            Some(reopened),
            "repeated activation reuses the window"
        );
        let _ = app.update(Message::WindowClosed(original.unwrap()));
        assert_eq!(
            app.window,
            Some(reopened),
            "late close events do not close the new window"
        );
    }
}
