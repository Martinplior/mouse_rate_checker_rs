#![deny(unsafe_op_in_unsafe_fn)]

pub mod main_app;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub fn graceful_run<R>(
    f: impl FnOnce() -> R + std::panic::UnwindSafe,
) -> Result<R, Box<dyn std::any::Any + Send>> {
    std::panic::catch_unwind(f).map_err(|err| {
        let message = if let Some(err) = err.downcast_ref::<String>() {
            err.clone()
        } else if let Some(err) = err.downcast_ref::<&str>() {
            err.to_string()
        } else {
            format!("{:?}, type_id = {:?}", err, err.type_id())
        };
        rfd::MessageDialog::new()
            .set_title("错误")
            .set_level(rfd::MessageLevel::Error)
            .set_description(message)
            .show();
        err
    })
}
