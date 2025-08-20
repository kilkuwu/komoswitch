use std::io::{BufReader, Read};
use std::time::Duration;

use anyhow::Context;
use komorebi_client::{
    Notification, NotificationEvent, SocketMessage, State,
    WindowManagerEvent,
};
use winsafe::HWND;

use crate::msgs::UpdateWorkspaces;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceState {
    Empty,
    NonEmpty,
    Focused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub name: String,
    // pub idx: usize,
    pub state: WorkspaceState,
}

fn workspaces_from_state(state: State) -> anyhow::Result<Vec<Workspace>> {
    let monitor = state.monitors.focused().context("No focused monintor?")?;

    let focused_workspace = monitor.focused_workspace_idx();

    let workspaces = monitor.workspaces().iter().enumerate().map(|(idx, w)| {
        let name = w.name.clone().unwrap_or_else(|| (idx + 1).to_string());
        let state = if focused_workspace == idx {
            WorkspaceState::Focused
        } else if w.is_empty() {
            WorkspaceState::Empty
        } else {
            WorkspaceState::NonEmpty
        };
        Workspace {
            name,
            // idx,
            state
        }
    });

    Ok(workspaces.collect())
}

pub fn read_workspaces() -> anyhow::Result<Vec<Workspace>> {
    log::debug!("Reading komorebi workspaces");
    let response = komorebi_client::send_query(&SocketMessage::State)?;
    let state: State = serde_json::from_str(&response)?;
    workspaces_from_state(state)
}

#[cfg(debug_assertions)]
const SOCK_NAME: &str = "komorebi-switcher-debug.sock";
#[cfg(not(debug_assertions))]
const SOCK_NAME: &str = "komorebi-switcher.sock";

pub fn listen_for_workspaces(hwnd: HWND) -> anyhow::Result<()> {
    let socket = loop {
        match komorebi_client::subscribe(SOCK_NAME) {
            Ok(socket) => break socket,
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        };
    };

    log::debug!("Listenting for messages from komorebi");

    for client in socket.incoming() {
        log::debug!("New loop for socket client {client:?}");
        log::error!("Testing error handling");
        let client = match client {
            Ok(client) => client,
            Err(e) => {
                if e.raw_os_error().expect("could not get raw os error") == 109 {
                    log::warn!("komorebi is no longer running");

                    let mut output = std::process::Command::new("cmd.exe")
                        .args(["/C", "komorebic.exe", "subscribe-socket", SOCK_NAME])
                        .output()?;

                    while !output.status.success() {
                        log::warn!(
                            "komorebic.exe failed with error code {:?}, retrying in 5 seconds...",
                            output.status.code()
                        );

                        std::thread::sleep(Duration::from_secs(5));

                        output = std::process::Command::new("cmd.exe")
                            .args(["/C", "komorebic.exe", "subscribe-socket", SOCK_NAME])
                            .output()?;
                    }

                    log::warn!("reconnected to komorebi");
                } else {
                    log::error!("Error while receiving a client from komorebi: {e}");
                }
                continue;
            }
        };
        log::debug!("Client acquired");

        let mut buffer = Vec::new();
        let mut reader = BufReader::new(client);
        log::debug!("buffer reader acquired");

        // this is when we know a shutdown has been sent
        if matches!(reader.read_to_end(&mut buffer), Ok(0)) {
            log::debug!("Disconnected from komorebi");
            std::thread::sleep(Duration::from_secs(30));

            // keep trying to reconnect to komorebi
            let connect_message = SocketMessage::AddSubscriberSocket(SOCK_NAME.into());
            while let Err(e) = komorebi_client::send_message(&connect_message) {
                log::debug!("Failed to reconnect to komorebi: {e}");
                std::thread::sleep(Duration::from_secs(1));
            }

            log::debug!("Reconnected to komorebi");

            continue;
        }

        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buffer) else {
            log::error!("Failed to parse JSON from komorebi");
            continue;
        };

        log::debug!(
            "Received an event from komorebi: {}",
            value
                .get("event")
                .and_then(|o| o.as_object())
                .and_then(|o| o.get("type"))
                .map(|v| v.to_string())
                .unwrap_or_default()
        );

        let Ok(notification) = serde_json::from_value::<Notification>(value) else {
            log::error!("Failed to parse notification from komorebi");
            continue;
        };

        log::debug!("Received notification");

        let should_update = match notification.event {
            NotificationEvent::Socket(notif) if should_update_sm(&notif) => true,
            NotificationEvent::WindowManager(notif) if should_update_wme(&notif) => true,
            _ => false,
        };
        log::debug!("Should update: {}", should_update);

        if !should_update {
            log::debug!("Skipping update for this notification");
            continue;
        }

        let new_workspaces = match workspaces_from_state(notification.state) {
            Ok(workspaces) => workspaces,
            Err(e) => {
                log::error!("Failed to read workspaces from state: {e}");
                continue;
            }
        };

        unsafe { hwnd.PostMessage(UpdateWorkspaces::to_wmdmsg(new_workspaces))?; }

        log::debug!("Updated workspaces");
    }

    log::debug!("Exiting komorebi listener loop");

    Ok(())
}

fn should_update_wme(notif: &WindowManagerEvent) -> bool {
    matches!(
        notif,
        WindowManagerEvent::Cloak(..)
            | WindowManagerEvent::Uncloak(..)
            | WindowManagerEvent::Destroy(..) // | WindowManagerEvent::FocusChange(..)
                                              // | WindowManagerEvent::Hide(..)
    )
}

fn should_update_sm(notif: &SocketMessage) -> bool {
    matches!(
        notif,
        SocketMessage::FocusWorkspaceNumber(_)
            | SocketMessage::FocusMonitorNumber(_)
            | SocketMessage::FocusMonitorWorkspaceNumber(..)
            | SocketMessage::FocusNamedWorkspace(_)
            | SocketMessage::FocusWorkspaceNumbers(_)
            | SocketMessage::CycleFocusMonitor(_)
            | SocketMessage::CycleFocusWorkspace(_)
            | SocketMessage::ReloadConfiguration
            | SocketMessage::ReplaceConfiguration(_)
            | SocketMessage::CompleteConfiguration
            | SocketMessage::ReloadStaticConfiguration(_)
            | SocketMessage::MoveContainerToMonitorNumber(_)
            | SocketMessage::MoveContainerToMonitorWorkspaceNumber(..)
            | SocketMessage::MoveContainerToNamedWorkspace(_)
            | SocketMessage::MoveContainerToWorkspaceNumber(_)
            | SocketMessage::MoveWorkspaceToMonitorNumber(_)
            | SocketMessage::CycleMoveContainerToMonitor(_)
            | SocketMessage::CycleMoveContainerToWorkspace(_)
            | SocketMessage::CycleMoveWorkspaceToMonitor(_)
            | SocketMessage::CloseWorkspace
            | SocketMessage::SendContainerToMonitorNumber(_)
            | SocketMessage::SendContainerToMonitorWorkspaceNumber(..)
            | SocketMessage::SendContainerToNamedWorkspace(_)
            | SocketMessage::SendContainerToWorkspaceNumber(_)
            | SocketMessage::CycleSendContainerToMonitor(_)
            | SocketMessage::CycleSendContainerToWorkspace(_) // | SocketMessage::Hide(_)
                                                              // | SocketMessage::Minimize(_)
                                                              // | SocketMessage::Show(_)
                                                              // | SocketMessage::TitleUpdate(_)
    )
}
