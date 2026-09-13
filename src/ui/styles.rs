use iced::widget::{container, text};
use iced::{Element, Length};

use super::Message;

pub(super) fn sbar_style(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(
            theme.extended_palette().background.strong.color,
        )),
        ..Default::default()
    }
}

pub(super) fn mmon_style(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(
            theme.extended_palette().background.stronger.color,
        )),
        ..Default::default()
    }
}

pub(super) fn panel_style(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(
            theme.extended_palette().background.weak.color,
        )),
        ..Default::default()
    }
}

fn bordered_base(theme: &iced::Theme, radius: f32) -> iced::widget::container::Style {
    let bg = theme.extended_palette().background;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(bg.base.color)),
        border: iced::Border {
            color: bg.strong.color,
            width: 1.0,
            radius: radius.into(),
        },
        ..Default::default()
    }
}

pub(super) fn editor_style(theme: &iced::Theme) -> iced::widget::container::Style {
    bordered_base(theme, 4.0)
}

pub(super) fn dialog_style(theme: &iced::Theme) -> iced::widget::container::Style {
    bordered_base(theme, 8.0)
}

pub(super) fn modal_bg(theme: &iced::Theme) -> iced::widget::container::Style {
    let palette = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(palette.background.base.color)),
        border: iced::Border {
            color: palette.primary.base.color,
            width: 2.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn separator<'a>() -> Element<'a, Message> {
    container(text("").size(1.0))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                theme.extended_palette().background.strong.color,
            )),
            ..Default::default()
        })
        .into()
}
