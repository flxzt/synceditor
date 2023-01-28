mod connection;
mod state;
mod ui;

use std::time::Duration;

use connection::{AppMessage, ConnectionState};
use druid::AppLauncher;
use state::{AppState, Delegate, TOPIC_NAME_INIT};
use ui::new_main_window;

use crate::connection::WireMessage;

fn main() {
    let (sender, recv) = futures::channel::mpsc::unbounded();

    let launcher = AppLauncher::with_window(new_main_window()).delegate(Delegate {
        sender: sender.clone(),
    });

    let ext_handle = launcher.get_external_handle();

    std::thread::spawn(move || {
        // Dirty hack so the connection only starts to run after app startup
        std::thread::sleep(Duration::from_millis(500));

        async_std::task::block_on(async move {
            let app_msg_handler = |message: AppMessage| {
                let sender = sender.clone();

                ext_handle.add_idle_callback(move |state: &mut AppState| match message {
                    AppMessage::ReceivedWireMessage(data) => {
                        let m = match serde_json::from_slice(&data) {
                            Ok(m) => m,
                            Err(e) => {
                                eprintln!("failed to deserialize received wire message, Err: {e:?}");
                                return;
                            }
                        };

                        if let Err(e) = state
                            .buffer
                            .handle_wire_message(m, sender.clone())
                        {
                            eprintln!("failed to handle received wire message data, Err {e:?}");
                        };
                    }
                    AppMessage::AddLogEntry(entry) => state.add_log_entry(entry),
                    AppMessage::PeerAdded((_, _)) => {
                        if let Err(e) = sender.unbounded_send(connection::ConnectionMessage::Send(
                            WireMessage::SyncDoc(state.buffer.doc_save()),
                        )) {
                            state.add_log_entry(format!(
                                "failed to send `RequestMerge` message after a peer was added, Err: {e:?}"
                            ));
                        }
                    }
                    AppMessage::PeerRemoved((_, _)) => {
                    }
                });
            };

            let mut connection = ConnectionState::init(TOPIC_NAME_INIT, app_msg_handler)
                .await
                .unwrap();

            connection.run(recv, app_msg_handler).await;
        });
    });

    launcher
        .log_to_console()
        .launch(AppState::new())
        .expect("Failed to launch application");
}
