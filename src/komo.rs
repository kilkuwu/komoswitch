use std::io::{BufReader, Read};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Context;
use komorebi_client::{Notification, Ring, SocketMessage, State, SubscribeOptions, Workspace};
use winsafe::HWND;

use crate::msgs::UpdateWorkspaces;

fn workspaces_from_state(
    state: State,
) -> anyhow::Result<Ring<Workspace>> {
    let monitor = state.monitors.focused().context("No focused monintor?")?;

    Ok(monitor.workspaces.clone())
}

pub fn read_workspaces() -> anyhow::Result<Ring<Workspace>> {
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
        log::debug!("Listenting for messages from komorebi...");

        for client in socket.incoming() {
            let client = match client {
                Ok(client) => client,
                Err(e) => {
                    log::error!("Failed to get komorebi event subscription: {e}");
                    continue;
                }
            };

            if let Err(error) = client.set_read_timeout(Some(Duration::from_secs(1))) {
                log::error!("Error when setting read timeout: {}", error)
            }

            let mut buffer = Vec::new();
            let mut reader = BufReader::new(client);

            // this is when we know a shutdown has been sent
            if matches!(reader.read_to_end(&mut buffer), Ok(0)) {
                log::info!("Disconnected from komorebi!");

                // keep trying to reconnect to komorebi
                while komorebi_client::send_message(&SocketMessage::AddSubscriberSocket(
                    SOCK_NAME.to_string(),
                ))
                .is_err()
                {
                    log::info!("Attempting to reconnect to komorebi...");
                    std::thread::sleep(Duration::from_secs(3));
                }

                log::info!("Reconnected to komorebi!");
                continue;
            }

            let notification_str = match String::from_utf8(buffer) {
                Ok(notification_str) => notification_str,
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

            log::info!(
                "Received notification from komorebi: {:?}",
                notification.event
            );

            // Always update because we have filtered state changes

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
    });

    Ok(handle)
}
