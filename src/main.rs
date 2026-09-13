use iced::Size;

const WINDOW_ICON: &[u8] = include_bytes!("../assets/midi-does-256.png");

fn main() -> iced::Result {
    // Hide-to-tray needs the X11 backend: native Wayland has no way to hide a
    // window or remove it from the taskbar. When XWayland is available
    // (DISPLAY is set), prefer it so closing the window hides it to the tray.
    if std::env::var_os("DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("WAYLAND_SOCKET");
    }

    env_logger::init();

    iced::application(
        midi_does::app::App::new,
        midi_does::app::App::update,
        midi_does::app::App::view,
    )
    .settings(iced::Settings {
        default_text_size: iced::Pixels(14.0),
        ..Default::default()
    })
    .title(midi_does::app::App::title)
    .theme(midi_does::app::App::theme)
    .subscription(midi_does::app::App::subscription)
    .window(iced::window::Settings {
        size: Size::new(900.0, 600.0),
        resizable: true,
        icon: iced::window::icon::from_file_data(WINDOW_ICON, None).ok(),
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "midi-does".into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .exit_on_close_request(false)
    .run()
}
