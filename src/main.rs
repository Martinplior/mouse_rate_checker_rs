#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app::MainApp;

mod app;
mod msg_hook;

fn main() {
    let _ = std::panic::catch_unwind(|| MainApp::new().run()).map_err(|err| {
        let message = if let Some(err) = err.downcast_ref::<String>() {
            err.clone()
        } else {
            format!("{:?}, type_id = {:?}", err, err.type_id())
        };
        #[cfg(debug_assertions)]
        dbg!(&message);
        rfd::MessageDialog::new()
            .set_title("错误")
            .set_level(rfd::MessageLevel::Error)
            .set_description(message)
            .show();
        err
    });
}
