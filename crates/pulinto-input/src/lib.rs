//! Input interception and synthesis traits and implementations.

pub mod capture;
pub mod grab;
pub mod injector;
pub mod evdev;
pub mod x11;
pub mod xtest_unicode;



#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("X11 connection error: {0}")]
    X11Error(String),
    #[error("Failed to inject key: {0}")]
    InjectionError(String),
    #[error("Permission denied to /dev/uinput")]
    PermissionDenied,
}

pub trait InputBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn send_key_combination(&self, keycode: u8, modifiers: u16) -> Result<(), InputError>;
    fn send_backspaces(&self, count: usize) -> Result<(), InputError>;
    fn send_text_for_layout(
        &self,
        text: &str,
        layout: &pulinto_core::layout::Layout,
    ) -> Result<(), InputError>;
    fn send_left(&self) -> Result<(), InputError>;
    fn send_right(&self) -> Result<(), InputError>;
}

pub fn create_default_backend() -> Box<dyn InputBackend> {
    // Dynamic X11 keycode remapping is not a safe text transport. Chromium and
    // some GTK clients can interpret a remapped key by its cached/physical
    // identity, turning individual letters into digits. Use the kernel virtual
    // keyboard, whose scan codes follow the same path as ordinary typing.
    match evdev::EvdevInputBackend::new() {
        Ok(backend) => return Box::new(backend),
        Err(error) => eprintln!("uinput unavailable ({error})"),
    }

    // Fail closed: a missed correction is preferable to damaged input.
    Box::new(evdev::EvdevInputBackend::new_mock())
}
