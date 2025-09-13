use std::io::{BufReader, Read};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Context;
use komorebi_client::{
    Notification, NotificationEvent, SocketMessage, State, SubscribeOptions, WindowManagerEvent,
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
            state,
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

pub fn start_listen_for_workspaces(hwnd: HWND) -> anyhow::Result<JoinHandle<()>> {
    let socket = loop {
        match komorebi_client::subscribe_with_options(
            SOCK_NAME,
            SubscribeOptions {
                filter_state_changes: true,
            },
        ) {
            Ok(socket) => break socket,
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        };
    };

    log::info!("Subscribed to komorebi events");

    let handle = std::thread::spawn(move || {
        log::debug!("Listenting for messages from komorebi");

        for client in socket.incoming() {
            log::debug!("New loop for socket client {client:?}");

            let client = match client {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to get komorebi event subscription: {e}");
                    continue;
                }
            };
            log::debug!("Client acquired");

            if let Err(error) = client.set_read_timeout(Some(Duration::from_secs(1))) {
                log::error!("Error when setting read timeout: {}", error)
            }

            log::debug!("Read timeout set");

            let mut buffer = Vec::new();
            let mut reader = BufReader::new(client);

            // this is when we know a shutdown has been sent
            if matches!(reader.read_to_end(&mut buffer), Ok(0)) {
                log::info!("disconnected from komorebi");

                // keep trying to reconnect to komorebi
                while komorebi_client::send_message(&SocketMessage::AddSubscriberSocket(
                    SOCK_NAME.to_string(),
                ))
                .is_err()
                {
                    log::info!("Attempting to reconnect to komorebi");
                    std::thread::sleep(Duration::from_secs(2));
                }

                log::info!("reconnected to komorebi");
                continue;
            }

            log::debug!("Read {} bytes from komorebi", buffer.len());

            let notification_str = match String::from_utf8(buffer) {
                Ok(notification_str) => 
                    notification_str,
                Err(e) => {
                    log::error!("Failed to parse komorebi notification string as utf8: {e}");
                    continue;
                }
            };

            let notification = match serde_json::from_str::<Notification>(&notification_str) {
                Ok(notification) => notification,
                Err(e) => {
                    log::error!("Failed to parse komorebi notification string as json: {e}");
                    continue;
                }
            };

            log::info!("Received notification from komorebi: {:?}", notification.event);

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

            unsafe {
                hwnd.PostMessage(UpdateWorkspaces::to_wmdmsg(new_workspaces))
                    .ok();
            }

            log::debug!("Posted message to update workspaces");
        }

        log::debug!("Exiting komorebi listener loop");
    });

    Ok(handle)
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
