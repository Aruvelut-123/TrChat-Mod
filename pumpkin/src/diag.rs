//! Diagnostics that cannot take the plugin down.
//!
//! Writing to the guest's standard streams is **fatal** on a real Pumpkin
//! server: the host does not wire `wasi:cli/stderr` for plugin instances, so
//! `std`'s `eprintln!` fails its write and panics with `failed printing to
//! stderr` — and that panic aborts the whole component. A smoke test on a real
//! server caught exactly this: an `on_load` abort while registering permissions,
//! where the underlying error was merely "this node is already registered".
//!
//! Every diagnostic therefore goes through the host's `pumpkin:plugin/logging`
//! interface (`pumpkin-plugin-wit/v0.1/logging.wit`), which lands in the server
//! log with a level and cannot panic.

/// Reports an informational message to the server log.
pub fn info(message: impl AsRef<str>) {
    emit(pumpkin_plugin_api::logging::LogLevel::Info, message.as_ref());
}

/// Reports a warning to the server log.
pub fn warn(message: impl AsRef<str>) {
    emit(pumpkin_plugin_api::logging::LogLevel::Warn, message.as_ref());
}

#[cfg(target_arch = "wasm32")]
fn emit(level: pumpkin_plugin_api::logging::LogLevel, message: &str) {
    pumpkin_plugin_api::logging::log(level, message);
}

#[cfg(not(target_arch = "wasm32"))]
fn emit(level: pumpkin_plugin_api::logging::LogLevel, message: &str) {
    // Host builds (`cargo test`) run on a real OS where stderr always works and
    // the WIT logging import is not reachable.
    eprintln!("[trchat] {level:?}: {message}");
}
