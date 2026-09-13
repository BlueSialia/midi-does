use std::sync::mpsc;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Icon, ToolTip};

#[derive(Debug, Clone)]
pub enum TrayMessage {
    ShowWindow,
    Quit,
}

pub struct TrayState {
    pub tooltip: String,
    pub sender: mpsc::Sender<TrayMessage>,
}

impl ksni::Tray for TrayState {
    fn icon_name(&self) -> String {
        "midi-does".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![]
    }

    fn title(&self) -> String {
        "midi-does".into()
    }

    fn id(&self) -> String {
        "midi-does".into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "midi-does".into(),
            description: self.tooltip.clone(),
            icon_name: "midi-does".into(),
            icon_pixmap: vec![],
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::MenuItem::Standard(StandardItem {
                label: "Show".into(),
                icon_name: "window".into(),
                enabled: true,
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayMessage::ShowWindow);
                }),
                ..Default::default()
            }),
            ksni::MenuItem::Separator,
            ksni::MenuItem::Standard(StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                enabled: true,
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayMessage::Quit);
                }),
                ..Default::default()
            }),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayMessage::ShowWindow);
    }
}

pub fn spawn(tooltip: String) -> mpsc::Receiver<TrayMessage> {
    let (tx, rx) = mpsc::channel();

    let tray = TrayState {
        tooltip,
        sender: tx,
    };

    if let Err(e) = tray.spawn() {
        log::error!("Failed to spawn tray service: {e}");
    }

    rx
}
