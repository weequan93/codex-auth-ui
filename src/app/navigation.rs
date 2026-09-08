//! Local-only account search, filtering, and toolbar presentation.
use super::*;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccountFilter {
    #[default]
    All,
    Selected,
    Attention,
}

impl AccountHubApp {
    pub(super) fn filtered_accounts(&self) -> Vec<AccountRecord> {
        let query = self.search.trim().to_lowercase();
        let selected = self.registry.active_account_key.as_deref();
        self.registry
            .accounts
            .iter()
            .filter(|account| {
                let matches_filter = match self.account_filter {
                    AccountFilter::All => true,
                    AccountFilter::Selected => selected == Some(account.account_key.as_str()),
                    AccountFilter::Attention => self.errors.contains_key(&account.account_key),
                };
                matches_filter
                    && (query.is_empty()
                        || [
                            account.display_name(),
                            account.email.as_str(),
                            account.display_plan(),
                        ]
                        .iter()
                        .any(|value| value.to_lowercase().contains(&query)))
            })
            .cloned()
            .collect()
    }

    pub(super) fn selected_navigation_target(&self) -> Option<&str> {
        self.registry.active_account_key.as_deref().filter(|key| {
            self.registry
                .accounts
                .iter()
                .any(|account| account.account_key == *key)
        })
    }

    pub(super) fn go_to_selected(&mut self) {
        if let Some(key) = self.selected_navigation_target().map(str::to_owned) {
            self.search.clear();
            self.account_filter = AccountFilter::All;
            self.scroll_to_account = Some(key);
        }
    }

    pub(super) fn account_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 49.0).max(80.0);
            let response = ui.add_sized(
                [width, 30.0],
                egui::TextEdit::singleline(&mut self.search)
                    .id_salt("account-search")
                    .background_color(CANVAS)
                    .hint_text("Search name, email, or plan")
                    .margin(egui::vec2(10.0, 7.0)),
            );
            if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::F)) {
                response.request_focus();
            }
            if ui
                .add_enabled(!self.search.is_empty(), text_button("Clear", MUTED))
                .clicked()
            {
                self.search.clear();
            }
        });
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            for (filter, title) in [
                (AccountFilter::All, "All accounts"),
                (AccountFilter::Selected, "Selected"),
                (AccountFilter::Attention, "Attention"),
            ] {
                let selected = self.account_filter == filter;
                if ui
                    .add(
                        egui::Button::new(RichText::new(title).size(11.5).color(if selected {
                            ACCENT
                        } else {
                            MUTED
                        }))
                        .fill(if selected {
                            ACCENT_SOFT
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(7.0)
                        .min_size(Vec2::new(76.0, 27.0)),
                    )
                    .on_hover_text(if filter == AccountFilter::Attention {
                        "Accounts with a failed quota check or sign-in issue"
                    } else {
                        "Filter saved accounts"
                    })
                    .clicked()
                {
                    self.account_filter = filter;
                }
            }
            if ui
                .add_enabled(
                    self.selected_navigation_target().is_some(),
                    outline_button("Go to selected").min_size(Vec2::new(108.0, 27.0)),
                )
                .on_hover_text(
                    "Show all accounts and scroll to the selected account, keeping the saved order",
                )
                .clicked()
            {
                self.go_to_selected();
            }
        });
    }
}
