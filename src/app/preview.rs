//! Synthetic UI fixtures: no worker thread, credential access, or network requests.
use super::*;

impl AccountHubApp {
    #[cfg(feature = "visual-qa")]
    pub fn preview(context: &egui::Context, scenario: &str) -> Self {
        let mut app = fixture(context, scenario).0;
        if scenario == "tray" {
            // Native packaging QA: real tray, synthetic accounts, inert worker.
            let (events, receiver) = unbounded();
            app.app_events = receiver;
            install_tray_handlers(context, events);
            match build_tray(&app.registry) {
                Ok(tray) => app.tray = Some(tray),
                Err(error) => app.banner_error = Some(error.to_string()),
            }
        }
        app
    }
}

fn fixture(context: &egui::Context, scenario: &str) -> (AccountHubApp, Receiver<WorkerCommand>) {
    configure_style(context);
    let now = storage::now_seconds();
    let registry = serde_json::from_value(serde_json::json!({
        "schema_version": 4,
        "active_account_key": "personal::sample",
        "accounts": [
            {
                "account_key": "personal::sample", "chatgpt_account_id": "sample",
                "chatgpt_user_id": "personal", "email": "alex@example.com",
                "alias": "Personal", "plan": "plus", "auth_mode": "chatgpt", "created_at": now,
                "last_usage_at": now - 120,
                "last_usage": {
                    "primary": { "used_percent": 18, "resets_at": now + 8100 },
                    "secondary": { "used_percent": 72, "resets_at": now + 273600 }
                }
            },
            {
                "account_key": "studio::sample", "chatgpt_account_id": "sample",
                "chatgpt_user_id": "studio", "email": "alex@studio.example",
                "alias": "Studio", "plan": "business", "auth_mode": "chatgpt", "created_at": now,
                "last_usage_at": now - 300,
                "last_usage": {
                    "primary": { "used_percent": 91, "resets_at": now + 3900 },
                    "secondary": { "used_percent": 43, "resets_at": now + 273600 }
                }
            }
        ]
    }))
    .expect("valid synthetic account fixture");
    let (worker, commands) = WorkerHandle::preview();
    let (_, app_events) = unbounded();
    let mut app = AccountHubApp {
        registry,
        search: String::new(),
        account_filter: AccountFilter::All,
        scroll_to_account: None,
        worker,
        app_events,
        tray: None,
        visible: true,
        busy: HashSet::new(),
        errors: HashMap::new(),
        preferences: Preferences::default(),
        disclosure_for: None,
        switch_guard: None,
        remove_confirm: None,
        editing_alias: None,
        login_active: false,
        login_message: String::new(),
        banner_error: None,
        session_notice: None,
        restart_confirm: false,
        restarting: false,
        #[cfg(not(target_os = "macos"))]
        last_position: None,
        preview_mode: true,
    };
    let first_key = "personal::sample".to_owned();
    match scenario {
        "empty" => app.registry = Registry::default(),
        "single" => app.registry.accounts.truncate(1),
        "long" => {
            app.registry.accounts[0].alias =
                "Research and development workspace with a very long name".into();
            app.registry.accounts[0].email =
                "a.very.long.email.address.for.testing@subdomain.example.com".into();
        }
        "login" => {
            app.login_active = true;
            app.login_message = "Open https://auth.example.com/device\nEnter this one-time code: ABCD-EFGH\nContinue only if you started this login in Codex. Waiting for browser confirmation…".into();
        }
        "error" => {
            app.banner_error = Some("Could not reach the quota service. Check your connection and try again. Reference: https://example.com/support/network/connection/timeout".into());
            app.errors.insert(
                first_key,
                RefreshFailure {
                    kind: RefreshFailureKind::NeedsRelogin,
                    message: "Session expired".into(),
                },
            );
        }
        "rename" => app.editing_alias = Some((first_key, "Personal account".into())),
        "remove" => app.remove_confirm = Some(first_key),
        "disclosure" => app.disclosure_for = Some(first_key),
        "guard" => {
            app.switch_guard = Some(SwitchGuard {
                account_key: "studio::sample".into(),
                pids: vec![],
            })
        }
        "stale" => {
            let usage = app.registry.accounts[0].last_usage.as_mut().unwrap();
            usage.primary.as_mut().unwrap().resets_at = Some(now - 60);
        }
        "missing" => app.registry.accounts[0].last_usage = None,
        "api" => {
            app.registry.accounts[0].auth_mode = Some("apikey".into());
            app.registry.active_account_key = None;
        }
        "saved" => app.registry.active_account_key = None,
        "restart" => app.restart_confirm = true,
        "selected" => app.session_notice = Some(SessionNotice::Selected),
        "restarted" => app.session_notice = Some(SessionNotice::RelaunchRequested),
        "restart-failed" => app.session_notice = Some(SessionNotice::RestartFailed("The desktop app stayed open. Finish its prompts and try again. It was not force-closed.".into())),
        _ => {}
    }
    (app, commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{epaint::Shape, Event, Pos2, RawInput, Rect};

    #[test]
    fn search_preserves_display_and_saved_order_when_selection_changes() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "accounts");
        app.registry.active_account_key = Some("studio::sample".into());
        let original = app.registry.clone();
        assert_eq!(app.filtered_accounts()[0].account_key, "personal::sample");
        assert_eq!(app.filtered_accounts()[1].account_key, "studio::sample");
        for query in ["  STUDIO  ", "alex@studio", "BUSINESS"] {
            app.search = query.into();
            let matches = app.filtered_accounts();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].account_key, "studio::sample");
        }
        app.search = "not found".into();
        assert!(app.filtered_accounts().is_empty());
        assert_eq!(app.registry, original);
        assert!(commands.is_empty());
    }

    #[test]
    fn go_to_selected_clears_filters_and_scrolls_without_switching() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "accounts");
        // A middle account in a long list proves navigation is an actual scroll,
        // rather than reordering or showing only the selected account.
        let template = app.registry.accounts[0].clone();
        app.registry.accounts = (0..9)
            .map(|index| {
                let mut account = template.clone();
                account.account_key = format!("sample-{index}");
                account.alias = format!("Account {index}");
                account
            })
            .collect();
        app.registry.active_account_key = Some("sample-5".into());
        let original = app.registry.clone();
        app.search = "no matches".into();
        app.account_filter = AccountFilter::Attention;
        click(&mut app, &context, "Go to selected");
        let labels = texts(&settled(&mut app, &context));
        let (_, selected_rect) = labels
            .iter()
            .find(|(label, _)| label == "Account 5")
            .expect("selected card rendered");
        assert!(
            selected_rect.top() >= 158.0 && selected_rect.bottom() < 300.0,
            "selected account should be near the list top: {selected_rect:?}"
        );
        assert!(app.search.is_empty());
        assert!(app.account_filter == AccountFilter::All);
        assert!(app.scroll_to_account.is_none());
        assert_eq!(app.filtered_accounts().len(), 9);
        assert_eq!(app.filtered_accounts()[0].account_key, "sample-0");
        assert_eq!(app.registry, original);
        assert!(commands.is_empty());
    }

    #[test]
    fn go_to_selected_is_disabled_without_a_valid_selected_account() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "saved");
        app.search = "keep my query".into();
        click(&mut app, &context, "Go to selected");
        assert_eq!(app.search, "keep my query");
        assert!(app.scroll_to_account.is_none());
        app.registry.active_account_key = Some("removed-account".into());
        app.go_to_selected();
        assert!(app.scroll_to_account.is_none());
        assert!(commands.is_empty());
    }

    #[test]
    fn filters_and_empty_result_recovery_are_local_only() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "accounts");
        click(&mut app, &context, "Selected");
        assert_eq!(app.filtered_accounts().len(), 1);
        click(&mut app, &context, "Attention");
        assert!(app.filtered_accounts().is_empty());
        assert!(texts(&settled(&mut app, &context))
            .iter()
            .any(|(text, _)| text == "No matching accounts"));
        click(&mut app, &context, "Show all accounts");
        assert_eq!(app.filtered_accounts().len(), 2);
        app.errors.insert(
            "studio::sample".into(),
            RefreshFailure {
                kind: RefreshFailureKind::Network,
                message: "Sample error".into(),
            },
        );
        app.account_filter = AccountFilter::Attention;
        assert_eq!(app.filtered_accounts()[0].account_key, "studio::sample");
        assert!(commands.is_empty());
    }

    #[test]
    fn removed_accounts_do_not_leave_stale_attention_errors() {
        let context = egui::Context::default();
        let (mut app, _) = fixture(&context, "error");
        app.handle_worker_event(&context, WorkerEvent::RegistryLoaded(Registry::default()));
        assert!(app.errors.is_empty());
    }

    #[test]
    fn keyboard_search_and_escape_leave_panel_open() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "accounts");
        settled(&mut app, &context);
        frame(
            &mut app,
            &context,
            vec![Event::Key {
                key: egui::Key::F,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
        );
        // Also supply the held shortcut modifier in RawInput, as winit does.
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW_SIZE.into())),
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![Event::Key {
                    key: egui::Key::F,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |context| app.render(context),
        );
        frame(&mut app, &context, vec![Event::Text("Studio".into())]);
        assert_eq!(app.search, "Studio");
        frame(
            &mut app,
            &context,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.search.is_empty());
        assert!(app.visible);
        assert!(commands.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn queued_tray_transitions_use_native_result_not_a_second_toggle() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "accounts");
        let (sender, receiver) = unbounded();
        app.app_events = receiver;
        for visible in [false, true, false, true, true] {
            sender.send(AppEvent::VisibilityChanged(visible)).unwrap();
        }
        app.process_events(&context);
        assert!(app.visible);
        assert_eq!(
            commands
                .try_iter()
                .filter(|cmd| matches!(cmd, WorkerCommand::Reload))
                .count(),
            3
        );
        sender.send(AppEvent::VisibilityChanged(false)).unwrap();
        app.process_events(&context);
        assert!(!app.visible);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn escape_hides_and_menu_show_restores_without_losing_dialog_state() {
        let context = egui::Context::default();
        let (mut app, _) = fixture(&context, "accounts");
        frame(
            &mut app,
            &context,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert!(!app.visible);
        app.editing_alias = Some(("personal::sample".into(), "Unfinished name".into()));
        app.handle_menu(&context, "show");
        assert!(app.visible);
        assert_eq!(app.editing_alias.as_ref().unwrap().1, "Unfinished name");
    }

    fn frame(
        app: &mut AccountHubApp,
        context: &egui::Context,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW_SIZE.into())),
                events,
                ..Default::default()
            },
            |context| app.render(context),
        )
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, Rect)> {
        fn visit(shape: &Shape, found: &mut Vec<(String, Rect)>) {
            match shape {
                Shape::Text(text) => {
                    // Empty TextEdit galleys have no painted bounds.
                    if !text.galley.text().is_empty() {
                        found.push((text.galley.text().to_owned(), text.visual_bounding_rect()));
                    }
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| visit(shape, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for shape in &output.shapes {
            visit(&shape.shape, &mut found);
        }
        found
    }

    fn settled(app: &mut AccountHubApp, context: &egui::Context) -> egui::FullOutput {
        frame(app, context, vec![]);
        frame(app, context, vec![]);
        frame(app, context, vec![])
    }

    fn click(app: &mut AccountHubApp, context: &egui::Context, label: &str) {
        let output = settled(app, context);
        let (_, rect) = texts(&output)
            .into_iter()
            .find(|(text, rect)| text == label && rect.bottom() < WINDOW_HEIGHT && rect.top() > 0.0)
            .unwrap_or_else(|| panic!("visible action {label:?} missing"));
        let pos = rect.center();
        for pressed in [true, false] {
            frame(
                app,
                context,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn long_content_and_dialogs_fit_the_panel_with_footer_visible() {
        for scenario in [
            "accounts",
            "empty",
            "long",
            "login",
            "error",
            "rename",
            "remove",
            "disclosure",
            "guard",
            "stale",
            "missing",
            "api",
            "restart",
            "selected",
            "restarted",
            "restart-failed",
        ] {
            let context = egui::Context::default();
            let (mut app, _) = fixture(&context, scenario);
            let first_frame = frame(&mut app, &context, vec![]);
            let settled_frame = settled(&mut app, &context);
            for output in [first_frame, settled_frame] {
                let labels = texts(&output);
                for (text, rect) in &labels {
                    assert!(
                        rect.left() >= -1.0 && rect.right() <= WINDOW_WIDTH + 1.0,
                        "{scenario}: {text:?} overflows horizontally: {rect:?}"
                    );
                }
                if !matches!(scenario, "disclosure" | "guard" | "restart") {
                    for action in ["+  Add account", "Device code"] {
                        let (_, rect) = labels
                            .iter()
                            .find(|(text, _)| text == action)
                            .expect("footer action");
                        assert!(
                            rect.top() > 530.0 && rect.bottom() < 600.0,
                            "{scenario}: footer moved out of view: {rect:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn remove_requires_confirmation_and_escape_cancels_rename() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "single");
        let labels = texts(&settled(&mut app, &context));
        assert!(!labels
            .iter()
            .any(|(text, _)| text == "Rename" || text == "Remove"));
        click(&mut app, &context, "...");
        click(&mut app, &context, "Remove");
        assert!(commands.is_empty());
        assert_eq!(app.remove_confirm.as_deref(), Some("personal::sample"));
        click(&mut app, &context, "Cancel");
        assert!(app.remove_confirm.is_none());
        click(&mut app, &context, "...");
        click(&mut app, &context, "Rename");
        assert!(app.editing_alias.is_some());
        frame(
            &mut app,
            &context,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.editing_alias.is_none());
        assert!(app.visible);
        click(&mut app, &context, "...");
        click(&mut app, &context, "Remove");
        click(&mut app, &context, "Remove");
        assert!(
            matches!(commands.try_recv(), Ok(WorkerCommand::Remove { account_key }) if account_key == "personal::sample")
        );
    }

    #[test]
    fn selected_credentials_do_not_claim_a_verified_session() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "single");
        let labels = texts(&settled(&mut app, &context));
        assert!(labels.iter().any(|(text, _)| text == "Selected"));
        assert!(labels
            .iter()
            .any(|(text, _)| text == "Selected credentials"));
        assert!(!labels.iter().any(|(text, _)| text == "Active"
            || text == "Currently in use"
            || text == "Session account unverified"));
        click(&mut app, &context, "...");
        click(&mut app, &context, "Session & restart");
        assert!(app.restart_confirm);
        assert!(commands.is_empty());
        click(&mut app, &context, "Cancel");
        assert!(!app.restart_confirm);
        assert!(commands.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_restart_requires_explicit_confirmation() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "selected");
        click(&mut app, &context, "Restart guidance");
        assert!(commands.is_empty());
        click(&mut app, &context, "Restart now");
        assert!(app.restarting);
        assert!(!app.restart_confirm);
        assert!(
            matches!(commands.try_recv(), Ok(WorkerCommand::RestartDesktop { account_key }) if account_key == "personal::sample")
        );
        app.begin_login(false);
        app.request_refresh("personal::sample".into());
        assert!(commands.is_empty());
    }

    #[test]
    fn completed_restart_replaces_guidance_and_dismiss_stays_dismissed() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "selected");
        app.restarting = true;
        app.session_notice = Some(SessionNotice::Restarting);
        app.handle_worker_event(&context, WorkerEvent::DesktopRestarted);
        assert!(!app.restarting);
        assert_eq!(app.session_notice, Some(SessionNotice::RelaunchRequested));
        let labels = texts(&settled(&mut app, &context));
        assert!(labels
            .iter()
            .any(|(text, _)| text == "Desktop relaunch requested"));
        assert!(!labels
            .iter()
            .any(|(text, _)| text == "Restart guidance" || text.contains("unverified")));
        click(&mut app, &context, "Dismiss");
        app.handle_worker_event(&context, WorkerEvent::RegistryLoaded(app.registry.clone()));
        assert!(app.session_notice.is_none());
        assert!(commands.is_empty());
    }

    #[test]
    fn manual_restart_notice_can_be_dismissed_and_failures_still_explain_the_problem() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "selected");
        click(&mut app, &context, "Dismiss");
        assert!(app.session_notice.is_none());
        assert!(commands.is_empty());
        app.restarting = true;
        app.handle_worker_event(
            &context,
            WorkerEvent::DesktopRestartFailed("App declined to quit".into()),
        );
        app.handle_worker_event(&context, WorkerEvent::RegistryLoaded(app.registry.clone()));
        assert!(!app.restarting);
        assert_eq!(
            app.session_notice,
            Some(SessionNotice::RestartFailed("App declined to quit".into()))
        );
        let labels = texts(&settled(&mut app, &context));
        assert!(labels
            .iter()
            .any(|(text, _)| text == "Restart not completed"));
        assert!(labels
            .iter()
            .any(|(text, _)| text == "App declined to quit"));
    }

    #[test]
    fn refresh_shows_disclosure_and_switch_keeps_the_process_guard() {
        let context = egui::Context::default();
        let (mut app, commands) = fixture(&context, "saved");
        click(&mut app, &context, "Refresh");
        assert!(commands.is_empty());
        assert!(app.disclosure_for.is_some());
        click(&mut app, &context, "Cancel");
        click(&mut app, &context, "Switch account");
        assert!(matches!(
            commands.try_recv(),
            Ok(WorkerCommand::SwitchIntent { .. })
        ));
    }
}
