use super::*;
use crate::credentials::{Status, Storage};

#[derive(Default)]
pub(super) struct Credentials {
    pub status: Option<Result<Status, String>>,
    pub warning: Option<String>,
    pub busy: bool,
    pub confirm: bool,
    pub dirty: bool,
    confirm_exit: bool,
    allow_exit: bool,
}
impl App {
    pub(super) fn credential_close_pending(&self) -> bool {
        self.credentials.confirm_exit
    }
    pub(super) fn credential_close_guard(&mut self, ctx: &egui::Context) {
        if !self.credentials.dirty && !self.credentials.busy {
            self.credentials.confirm_exit = false;
            self.credentials.allow_exit = false;
        }
        if (self.credentials.dirty || self.credentials.busy)
            && !self.credentials.allow_exit
            && ctx.input(|input| input.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.credentials.confirm_exit = true;
        }
        if self.credentials.confirm_exit {
            egui::Window::new(tr("Credential storage needs attention")).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(if self.credentials.busy { "Credential storage is still running. Wait before closing; interrupted sign-out may leave saved credentials in place." } else { "Closing can lose the newest sign-in credentials. Keep the app open and retry storage, or explicitly exit without them." });
                if ui.button(tr("Keep app open")).clicked() { self.credentials.confirm_exit = false; }
                if ui.button(tr("Exit without waiting or saving")).clicked() {
                    self.credentials.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    }
    pub(super) fn credential_label(&self) -> &'static str {
        match &self.credentials.status {
            Some(Ok(status)) if status.storage == Storage::Keyring => {
                "Sign-in uses OS Secret Service. Keyring failure never switches to a plaintext file."
            }
            Some(Ok(_)) => {
                "Sign-in uses a private local token file (0600), not encrypted by this app. You can migrate it to the OS keyring in Settings."
            }
            _ => {
                "Credential storage is not ready. Storage failures are reported; no automatic plaintext fallback is permitted."
            }
        }
    }
    pub(super) fn credential_notice(&mut self, ui: &mut egui::Ui) {
        if self.credentials.dirty {
            ui.colored_label(theme_error(), tr("Current sign-in has not been confirmed in persistent storage. Keep the app open and retry saving before exit."));
        }
        let warning = self.credentials.warning.clone().or_else(|| {
            self.credentials
                .status
                .as_ref()
                .and_then(|result| result.as_ref().err().cloned())
        });
        if let Some(warning) = warning {
            ui.colored_label(theme_error(), warning);
            if self.page != Page::Settings && ui.button(tr("Credential settings")).clicked() {
                self.navigate(Page::Settings);
                self.settings_section = SettingsSection::Privacy;
            }
            ui.add_space(12.);
        }
    }
    pub(super) fn credential_request(&mut self, request: Request) {
        self.credentials.busy = self.try_send(request);
    }
    pub(super) fn credential_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(self.credential_label());
        ui.label(tr("Keyring protection depends on your desktop provider. Configure and unlock its default collection before migration; this app never opens automatic unlock prompts."));
        let storage = self
            .credentials
            .status
            .as_ref()
            .and_then(|result| result.as_ref().ok());
        let legacy = storage.is_some_and(|status| status.storage == Storage::Legacy);
        let cleanup = storage.is_some_and(|status| status.cleanup_pending);
        let signed_out = storage.is_some_and(|status| status.signed_out);
        if self.credentials.busy {
            ui.spinner();
        }
        ui.add_enabled_ui(!self.credentials.busy, |ui| {
            if self.connected && legacy && cfg!(target_os = "linux") {
                if self.credentials.confirm {
                    ui.label(tr("Move this sign-in to the OS keyring? The new copy is read-verified before the mode changes and the legacy file is removed. Older player versions cannot read this sign-in; do not run them against this profile after migration."));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("Confirm keyring migration")).clicked() && let Some(user) = self.account {
                            self.credentials.confirm = false;
                            self.credential_request(Request::MigrateCredentials { user });
                        }
                        if ui.button(tr("Cancel")).clicked() { self.credentials.confirm = false; }
                    });
                } else if ui.button(tr("Move sign-in to OS keyring…")).clicked() { self.credentials.confirm = true; }
            }
            if self.connected {
                if ui.button(tr("Retry saving current sign-in")).clicked() { self.credential_request(Request::RetryCredentialSave); }
            } else if !signed_out && ui.button(tr("Retry loading saved sign-in")).clicked() { self.credential_request(Request::ReloadCredentials); }
            if cleanup && ui.button(tr("Retry saved-credential cleanup")).clicked() { self.credential_request(Request::CleanupCredentials); }
            if ui.button(if self.connected { "Sign out and remove saved sign-in" } else { "Remove saved sign-in" }).clicked() { self.logout(); }
        });
        ui.label(tr("If deletion is blocked by a locked keyring, the signed-out marker prevents automatic sign-in while cleanup remains pending. Failed settings writes are reported and need retrying."));
    }
}

#[cfg(test)]
#[path = "credential_tests.rs"]
mod tests;
