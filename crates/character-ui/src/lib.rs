//! MVP-0: a standalone layer-shell drawer with a static mascot and manual events.

mod mascot;
mod model;

use std::time::{Duration, Instant};

use cosmic::{
    Application, Element,
    app::{Core, Settings, Task},
    iced::{
        self, Alignment, Border, Color, Length, Subscription,
        platform_specific::{
            runtime::wayland::layer_surface::SctkLayerSurfaceSettings,
            shell::commands::layer_surface::{
                Anchor, KeyboardInteractivity, Layer, get_layer_surface, set_size,
            },
        },
        window,
    },
    widget::{self, button, column, container, row, text},
};
use model::{Drawer, Input, ManualEvent};

/// Opens the drawer on the active Wayland output. Must run on the main thread.
///
/// No microphone, system watcher, action executor or LLM is started by MVP-0.
pub fn run() -> iced::Result {
    cosmic::app::run::<Character>(
        Settings::default()
            .no_main_window(true)
            .exit_on_close(false)
            .theme(cosmic::Theme::dark()),
        (),
    )
}

struct Character {
    core: Core,
    surface: window::Id,
    drawer: Drawer,
}

#[derive(Debug, Clone)]
enum Message {
    Input(Input),
    Pointer { surface: window::Id, inside: bool },
    Quit,
}

fn pointer_event(
    event: iced::Event,
    _status: iced::event::Status,
    surface: window::Id,
) -> Option<Message> {
    let inside = match event {
        iced::Event::Mouse(iced::mouse::Event::CursorEntered) => true,
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => false,
        _ => return None,
    };
    Some(Message::Pointer { surface, inside })
}

fn surface_settings(id: window::Id, size: (u32, u32)) -> SctkLayerSurfaceSettings {
    SctkLayerSurfaceSettings {
        id,
        namespace: Character::APP_ID.into(),
        layer: Layer::Top,
        anchor: Anchor::TOP,
        keyboard_interactivity: KeyboardInteractivity::None,
        exclusive_zone: 0,
        size: Some((Some(size.0), Some(size.1))),
        ..Default::default()
    }
}

impl Application for Character {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "org.character.Character";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, (): ()) -> (Self, Task<Message>) {
        let surface = window::Id::unique();
        let drawer = Drawer::default();
        let task = get_layer_surface(surface_settings(surface, drawer.size()));
        tracing::info!("starting manual drawer; microphone and system watchers are off");
        (
            Self {
                core,
                surface,
                drawer,
            },
            task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Quit => iced::exit(),
            Message::Pointer { surface, inside } => {
                if surface == self.surface {
                    self.drawer.update(
                        if inside {
                            Input::PointerEntered
                        } else {
                            Input::PointerLeft
                        },
                        Instant::now(),
                    );
                }
                Task::none()
            }
            Message::Input(input) => {
                let old_size = self.drawer.size();
                self.drawer.update(input, Instant::now());
                if let Input::Manual(event) = input {
                    tracing::info!(?event, emotion = ?self.drawer.emotion, "manual demo event");
                }
                let size = self.drawer.size();
                if old_size != size {
                    tracing::debug!(state = ?self.drawer.state, ?size, "drawer resized");
                    set_size(self.surface, Some(size.0), Some(size.1))
                } else {
                    Task::none()
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Widget hover callbacks can miss a Wayland surface leave (no cursor position).
        let pointer = iced::event::listen_with(pointer_event);
        let timeout = if self.drawer.deadline.is_some() {
            iced::time::every(Duration::from_millis(250))
                .map(|now| Message::Input(Input::Tick(now)))
        } else {
            Subscription::none()
        };
        Subscription::batch([pointer, timeout])
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        (id == self.surface).then_some(Message::Quit)
    }

    fn view(&self) -> Element<'_, Message> {
        self.view_window(self.surface)
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        let accent = mascot::accent(self.drawer.emotion);
        let content: Element<'_, Message> = if self.drawer.expanded() {
            let header = row([])
                .push(text::heading("Fen"))
                .push(text::caption("ДЕМО-РЕЖИМ").class(cosmic::theme::Text::Color(accent)))
                .push(widget::Space::new().width(Length::Fill))
                .push(
                    text::caption("●  МИКРОФОН ВЫКЛ.")
                        .class(cosmic::theme::Text::Color(Color::from_rgb8(91, 215, 187))),
                )
                .push(button::text("Свернуть").on_press(Message::Input(Input::Toggle)))
                .push(button::text("Выход").on_press(Message::Quit))
                .align_y(Alignment::Center)
                .spacing(10);

            let bubble: Element<'_, Message> = match self.drawer.tooltip {
                Some(tooltip) => container(
                    column([])
                        .push(
                            text::heading(tooltip.title).class(cosmic::theme::Text::Color(accent)),
                        )
                        .push(text::body(tooltip.body))
                        .spacing(10),
                )
                .padding(16)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Color::from_rgb8(23, 36, 57).into()),
                    border: Border {
                        color: Color { a: 0.45, ..accent },
                        width: 1.0,
                        radius: 18.0.into(),
                    },
                    ..Default::default()
                })
                .into(),
                None => container(
                    column([])
                        .push(text::heading("Рядом").class(cosmic::theme::Text::Color(accent)))
                        .push(text::body(
                            "Выберите событие — Фен покажет реакцию. Система не отслеживается.",
                        ))
                        .spacing(10),
                )
                .padding(16)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Color::from_rgb8(23, 36, 57).into()),
                    border: Border {
                        color: Color { a: 0.45, ..accent },
                        width: 1.0,
                        radius: 18.0.into(),
                    },
                    ..Default::default()
                })
                .into(),
            };
            let portrait = row([])
                .push(
                    widget::image::Image::new(mascot::image(self.drawer.emotion))
                        .width(176)
                        .height(158)
                        .content_fit(iced::ContentFit::Contain),
                )
                .push(bubble)
                .align_y(Alignment::Center)
                .spacing(16);
            let controls = ManualEvent::ALL
                .into_iter()
                .fold(row([]).spacing(10), |row, event| {
                    row.push(
                        button::standard(event.label())
                            .on_press(Message::Input(Input::Manual(event)))
                            .width(Length::Fill),
                    )
                });
            column([])
                .push(header)
                .push(portrait)
                .push(
                    text::caption("ДЕМО-СОБЫТИЯ")
                        .class(cosmic::theme::Text::Color(Color::from_rgb8(142, 160, 185))),
                )
                .push(controls)
                .spacing(10)
                .padding(16)
                .into()
        } else {
            button::custom(
                row([])
                    .push(
                        widget::image::Image::new(mascot::calm())
                            .width(32)
                            .height(32)
                            .content_fit(iced::ContentFit::Contain),
                    )
                    .push(text::body("Fen"))
                    .push(widget::Space::new().width(Length::Fill))
                    .push(text::caption("⌄").class(cosmic::theme::Text::Color(accent)))
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .on_press(Message::Input(Input::Toggle))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([2, 12])
            .into()
        };
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                text_color: Some(Color::from_rgb8(231, 239, 250)),
                background: Some(Color::from_rgba8(11, 19, 34, 0.97).into()),
                border: Border {
                    color: Color { a: 0.65, ..accent },
                    width: 1.2,
                    radius: iced::border::Radius {
                        top_left: 0.0,
                        top_right: 0.0,
                        bottom_left: 24.0,
                        bottom_right: 24.0,
                    },
                },
                ..Default::default()
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_surface_is_top_centered_without_focus_or_reserved_space() {
        let drawer = Drawer::default();
        let settings = surface_settings(window::Id::unique(), drawer.size());
        assert_eq!(settings.anchor, Anchor::TOP);
        assert_eq!(settings.layer, Layer::Top);
        assert_eq!(settings.keyboard_interactivity, KeyboardInteractivity::None);
        assert_eq!(settings.exclusive_zone, 0);
        assert_eq!(settings.size, Some((Some(160), Some(40))));
    }

    #[test]
    fn surface_leave_is_forwarded_even_when_a_widget_captures_the_event() {
        let surface = window::Id::unique();
        for status in [iced::event::Status::Ignored, iced::event::Status::Captured] {
            assert!(matches!(
                pointer_event(iced::Event::Mouse(iced::mouse::Event::CursorLeft), status, surface),
                Some(Message::Pointer { surface: id, inside: false }) if id == surface
            ));
        }
    }

    #[test]
    fn other_surfaces_do_not_change_the_drawer_timeout() {
        let (mut app, _) = Character::init(Core::default(), ());
        app.drawer.update(Input::Toggle, Instant::now());
        let deadline = app.drawer.deadline;
        let _ = app.update(Message::Pointer {
            surface: window::Id::unique(),
            inside: true,
        });
        assert_eq!(app.drawer.deadline, deadline);
        let _ = app.update(Message::Pointer {
            surface: app.surface,
            inside: true,
        });
        assert!(app.drawer.deadline.is_none());
        let _ = app.update(Message::Pointer {
            surface: app.surface,
            inside: false,
        });
        assert!(app.drawer.deadline.is_some());
    }
}
