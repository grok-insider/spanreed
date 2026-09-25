//! Tray context menu: its items and the action each one triggers.

use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};

use super::{
    MENU_AGENT_START, MENU_CHECK, MENU_DASHBOARD, MENU_ENSURE, MENU_LINK_SHARE, MENU_LOG,
    MENU_QUIT, MENU_REFRESH, MENU_SETTINGS, MENU_UNLINK_SHARE, MENU_UPDATE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Dashboard,
    Settings,
    Refresh,
    EnsureCapture,
    OpenLog,
    Share,
    Unlink,
    CheckUpdates,
    InstallUpdate,
    AgentHost,
    Quit,
}

/// Items whose label or enabled state changes at runtime, plus the id map.
pub(super) struct TrayMenu {
    pub(super) check: MenuItem,
    pub(super) update: MenuItem,
    pub(super) share: MenuItem,
    pub(super) unlink: MenuItem,
    pub(super) agent: MenuItem,
    ids: Vec<(MenuId, Action)>,
}

impl TrayMenu {
    /// The context menu handed to the tray icon and the handles kept here.
    pub(super) fn build() -> Result<(Menu, Self), String> {
        let item = |label: &str, enabled: bool| MenuItem::new(label, enabled, None);
        let dashboard = item(MENU_DASHBOARD, true);
        let settings = item(MENU_SETTINGS, true);
        let refresh = item(MENU_REFRESH, true);
        let ensure = item(MENU_ENSURE, true);
        let log = item(MENU_LOG, true);
        let share = item(MENU_LINK_SHARE, true);
        let unlink = item(MENU_UNLINK_SHARE, false);
        let check = item(MENU_CHECK, true);
        let update = item(MENU_UPDATE, false);
        let agent = item(MENU_AGENT_START, true);
        let quit = item(MENU_QUIT, true);
        let separator = PredefinedMenuItem::separator;
        let menu = Menu::new();
        menu.append_items(&[
            &dashboard,
            &settings,
            &separator(),
            &refresh,
            &ensure,
            &log,
            &separator(),
            &share,
            &unlink,
            &separator(),
            &check,
            &update,
            &separator(),
            &agent,
            &separator(),
            &quit,
        ])
        .map_err(|e| format!("menu: {e}"))?;
        let ids = vec![
            (dashboard.id().clone(), Action::Dashboard),
            (settings.id().clone(), Action::Settings),
            (refresh.id().clone(), Action::Refresh),
            (ensure.id().clone(), Action::EnsureCapture),
            (log.id().clone(), Action::OpenLog),
            (share.id().clone(), Action::Share),
            (unlink.id().clone(), Action::Unlink),
            (check.id().clone(), Action::CheckUpdates),
            (update.id().clone(), Action::InstallUpdate),
            (agent.id().clone(), Action::AgentHost),
            (quit.id().clone(), Action::Quit),
        ];
        Ok((
            menu,
            Self {
                check,
                update,
                share,
                unlink,
                agent,
                ids,
            },
        ))
    }

    pub(super) fn action(&self, id: &MenuId) -> Option<Action> {
        self.ids
            .iter()
            .find(|(item, _)| item == id)
            .map(|(_, action)| *action)
    }
}
