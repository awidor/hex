//! What the Linux and Windows tray menu offers: status, Show Hex, Paste
//! last dictation, the meeting items, the update, Enable / Disable Hex, and
//! Quit Hex. Each tray maps a picked [`TrayCommand`] onto its own runtime.

#[cfg(not(target_os = "macos"))]
pub(crate) use tray::*;

#[cfg(not(target_os = "macos"))]
mod tray {
    use crate::desktop::events::VoiceState;
    use crate::desktop::host::DesktopUpdateStatus;
    use crate::desktop::i18n::tr;

    /// What a tray menu item asks for.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum TrayCommand {
        Show,
        PasteLast,
        /// The update item: restart into it, download it, or switch channel.
        Update,
        ToggleListening,
        Quit,
        RecordMeeting,
        /// Stop & transcribe the recording meeting.
        StopMeeting,
        CopyLastMeeting,
    }

    impl TrayCommand {
        /// The command behind a menu item's id.
        pub(crate) fn from_id(id: &str) -> Option<Self> {
            Some(match id {
                "show" => Self::Show,
                "paste-last" => Self::PasteLast,
                "update" => Self::Update,
                "toggle" => Self::ToggleListening,
                "quit" => Self::Quit,
                "meeting-record" => Self::RecordMeeting,
                "meeting-stop" => Self::StopMeeting,
                "meeting-copy-last" => Self::CopyLastMeeting,
                _ => return None,
            })
        }
    }

    /// The meeting recorder, as the tray shows it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum MeetingTray {
        Idle,
        Starting,
        Recording,
        Transcribing,
    }

    /// Everything the tray shows.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(crate) struct TrayView {
        pub(crate) listening: bool,
        pub(crate) state: Option<VoiceState>,
        pub(crate) update: Option<String>,
        /// `None` while Meetings is off.
        pub(crate) meetings: Option<MeetingTray>,
    }

    impl TrayView {
        pub(crate) fn new(
            listening: bool,
            state: Option<VoiceState>,
            update: &DesktopUpdateStatus,
        ) -> Self {
            Self {
                listening,
                state: state.filter(|_| listening),
                update: update_label(update),
                meetings: None,
            }
        }

        fn recording(&self) -> bool {
            self.state == Some(VoiceState::Dictating)
        }

        fn transcribing(&self) -> bool {
            self.state == Some(VoiceState::Transcribing)
        }

        pub(crate) fn tooltip(&self) -> &'static str {
            if self.recording() {
                tr("Hex Dictation: recording")
            } else if self.transcribing() {
                tr("Hex Dictation: transcribing")
            } else if self.update.is_some() {
                tr("Hex Dictation: update available")
            } else {
                tr("Hex Dictation")
            }
        }

        pub(crate) fn menu(&self) -> Vec<rune::TrayItem> {
            use rune::TrayItem;
            let mut menu = Vec::new();
            let status = if self.recording() {
                Some(tr("Recording"))
            } else if self.transcribing() {
                Some(tr("Transcribing…"))
            } else {
                None
            };
            if let Some(status) = status {
                menu.push(TrayItem::action("status", status).enabled(false));
                menu.push(TrayItem::Separator);
            }
            menu.push(TrayItem::action("show", tr("Show Hex")));
            menu.push(
                TrayItem::action("paste-last", tr("Paste last dictation")).enabled(self.listening),
            );
            if let Some(meetings) = self.meetings {
                menu.push(TrayItem::Separator);
                // The disabled states take ids of their own: some tray hosts
                // (GNOME's AppIndicators) keep an item's disabled state when
                // the same id comes back enabled (#1287).
                menu.push(match meetings {
                    MeetingTray::Idle => TrayItem::action("meeting-record", tr("Record meeting")),
                    MeetingTray::Starting => {
                        TrayItem::action("meeting-starting", tr("Starting…")).enabled(false)
                    }
                    MeetingTray::Recording => {
                        TrayItem::action("meeting-stop", tr("Stop & transcribe"))
                    }
                    MeetingTray::Transcribing => {
                        TrayItem::action("meeting-transcribing", tr("Transcribing…")).enabled(false)
                    }
                });
                menu.push(TrayItem::action(
                    "meeting-copy-last",
                    tr("Copy last meeting transcript"),
                ));
            }
            menu.push(TrayItem::Separator);
            menu.extend(
                self.update
                    .as_ref()
                    .map(|label| TrayItem::action("update", label.clone())),
            );
            menu.push(TrayItem::action("toggle", tr("Enable / Disable Hex")));
            menu.push(TrayItem::action("quit", tr("Quit Hex")));
            menu
        }
    }

    /// The update the window's sidebar offers: Restart to update once a
    /// managed install has staged it, Download update for an install that
    /// cannot apply it itself, or the channel switch.
    fn update_label(status: &DesktopUpdateStatus) -> Option<String> {
        match status {
            DesktopUpdateStatus::ReadyToRestart => Some(tr("Restart to update").to_owned()),
            DesktopUpdateStatus::Available => Some(tr("Download update").to_owned()),
            DesktopUpdateStatus::SwitchAvailable { channel, version } => {
                Some(crate::updater::switch_label(channel, version))
            }
            _ => None,
        }
    }

    /// What the update item does for `status`, if it shows.
    pub(crate) fn update_action(
        status: &DesktopUpdateStatus,
    ) -> Option<crate::desktop::host::DesktopAction> {
        use crate::desktop::host::DesktopAction;
        match status {
            DesktopUpdateStatus::ReadyToRestart => Some(DesktopAction::RestartIntoUpdate),
            DesktopUpdateStatus::Available => Some(DesktopAction::OpenUpdate),
            DesktopUpdateStatus::SwitchAvailable { .. } => Some(DesktopAction::ConfirmUpdateSwitch),
            _ => None,
        }
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_ids_round_trip_and_show_the_state() {
        use crate::desktop::events::VoiceState;
        use crate::desktop::host::DesktopUpdateStatus;
        let view = TrayView::new(
            true,
            Some(VoiceState::Dictating),
            &DesktopUpdateStatus::ReadyToRestart,
        );
        let menu = view.menu();
        let ids: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                rune::TrayItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            ["status", "show", "paste-last", "update", "toggle", "quit"]
        );
        // The menu offers no speech models: there is one, reached online.
        assert_eq!(TrayCommand::from_id("model:scribe_v3"), None);
        assert_eq!(
            TrayCommand::from_id("paste-last"),
            Some(TrayCommand::PasteLast)
        );
        assert_eq!(TrayCommand::from_id("status"), None);
        assert_eq!(view.tooltip(), "Hex Dictation: recording");
        // Meetings on adds its items after Paste last dictation.
        let meeting = TrayView {
            meetings: Some(MeetingTray::Recording),
            ..view.clone()
        };
        let menu = meeting.menu();
        let ids: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                rune::TrayItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids[2..5],
            ["paste-last", "meeting-stop", "meeting-copy-last"]
        );
        assert_eq!(
            TrayCommand::from_id("meeting-stop"),
            Some(TrayCommand::StopMeeting)
        );
        // Disabled meeting states never reuse an enabled item's id (#1287).
        let meeting_id = |state| {
            TrayView {
                meetings: Some(state),
                ..view.clone()
            }
            .menu()
            .into_iter()
            .find_map(|item| match item {
                rune::TrayItem::Action { id, .. } if id.starts_with("meeting-") => Some(id),
                _ => None,
            })
            .unwrap()
        };
        let enabled = [
            meeting_id(MeetingTray::Idle),
            meeting_id(MeetingTray::Recording),
        ];
        for disabled in [MeetingTray::Starting, MeetingTray::Transcribing] {
            assert!(!enabled.contains(&meeting_id(disabled)));
        }
        let stopped = TrayView::new(
            false,
            Some(VoiceState::Dictating),
            &DesktopUpdateStatus::Current,
        );
        assert_eq!(stopped.tooltip(), "Hex Dictation");
    }
}
