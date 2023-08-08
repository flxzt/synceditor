mod connection;
mod state;
mod ui;

use connection::{app_message_handler, AppMessage, ConnectionState};
use druid::AppLauncher;
use state::{AppState, Delegate, TOPIC_NAME_INIT};
use std::time::Duration;
use ui::new_main_window;

fn main() {
    let (connection_msg_sender, connection_msg_receiver) = futures::channel::mpsc::unbounded();
    let launcher = AppLauncher::with_window(new_main_window()).delegate(Delegate {
        sender: connection_msg_sender.clone(),
    });
    let ext_handle = launcher.get_external_handle();

    std::thread::spawn(move || {
        // hack so the connection starts to run after app startup
        std::thread::sleep(Duration::from_millis(500));

        async_std::task::block_on(async move {
            let app_msg_handler = |message: AppMessage| {
                let connection_msg_sender = connection_msg_sender.clone();

                ext_handle.add_idle_callback(move |state: &mut AppState| {
                    app_message_handler(message, state, connection_msg_sender)
                });
            };

            let mut connection = ConnectionState::init(TOPIC_NAME_INIT, app_msg_handler)
                .await
                .unwrap();

            connection
                .run(connection_msg_receiver, app_msg_handler)
                .await;
        });
    });

    launcher
        .log_to_console()
        .launch(AppState::new())
        .expect("Failed to launch application");
}
