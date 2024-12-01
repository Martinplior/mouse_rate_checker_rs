#![deny(unsafe_op_in_unsafe_fn)]

pub mod global_listener_app;
pub mod main_app;

mod interprocess_channel;
mod msg_hook;

fn get_current_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap()
}

pub fn graceful_run<R>(
    f: impl FnOnce() -> R + std::panic::UnwindSafe,
) -> Result<R, Box<dyn std::any::Any + Send>> {
    std::panic::catch_unwind(f).map_err(|err| {
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
    })
}
