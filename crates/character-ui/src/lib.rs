//! Character's layer-shell drawer, with explicit push-to-talk voice interaction.

mod mascot;
mod model;

use std::time::{Duration, Instant};

use character_brain::{Brain, DEFAULT_PREAMBLE};
use character_types::{Mode, NetworkStatus};
use character_voice::{Recording, VoiceConfig, speak};
use cosmic::{
    Application, Element,
    app::{Core, Settings, Task},
    iced::{
        self, Alignment, Border, Color, Length, Subscription,
        futures::SinkExt,
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

struct NetworkWatcher;

///
/// Network connectivity is monitored; microphone access remains disabled until voice mode
/// is explicitly enabled.
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
    policy: character_core::Core,
    surface: window::Id,
    drawer: Drawer,
    mode: Mode,
    recording: Option<Recording>,
    voice_config: Option<VoiceConfig>,
    voice_busy: bool,
    status: String,
    transcript: Option<String>,
    answer: Option<String>,
    history: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
enum Message {
    Input(Input),
    NetworkStatus(NetworkStatus),
    Pointer { surface: window::Id, inside: bool },
    ToggleVoice,
    StartRecording,
    StopRecording,
    VoiceComplete(Result<VoiceReply, String>),
    SpeechComplete(Result<(), String>),
    Quit,
}

#[derive(Debug, Clone)]
struct VoiceReply {
    transcript: String,
    answer: String,
    piper_model: Option<std::path::PathBuf>,
}

impl Character {
    fn finish_recording(&mut self) -> Task<Message> {
        let Some(recording) = self.recording.take() else {
            return Task::none();
        };
        let Some(config) = self.voice_config.take() else {
            self.status = "Voice configuration was lost; recording discarded".into();
            return Task::none();
        };
        let history = self.history.clone();
        let recording_config = config.clone();
        let model = config.ollama_model.clone();
        self.voice_busy = true;
        self.status = "Transcribing and waiting for the local model…".into();
        Task::perform(
            async move {
                let transcript =
                    tokio::task::spawn_blocking(move || recording.transcribe(&recording_config))
                        .await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?;

                let brain = Brain::local_ollama(&model, DEFAULT_PREAMBLE);
                let answer = brain
                    .reply_with_context(&transcript, &history)
                    .await
                    .map_err(|error| error.to_string())?;

                Ok(VoiceReply {
                    transcript,
                    answer,
                    piper_model: config.piper_model,
                })
            },
            |result| Message::VoiceComplete(result).into(),
        )
    }
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
        tracing::info!("starting drawer; microphone is off until voice mode is explicitly enabled");
        (
            Self {
                core,
                policy: character_core::Core::new(Mode::default()),
                surface,
                drawer,
                mode: Mode::EventsOnly,
                recording: None,
                voice_config: None,
                voice_busy: false,
                status: "Microphone off".into(),
                transcript: None,
                answer: None,
                history: Vec::new(),
            },
            task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Quit => iced::exit(),
            Message::ToggleVoice => {
                if self.mode == Mode::Voice {
                    self.mode = Mode::EventsOnly;
                    self.recording = None;
                    self.voice_config = None;
                    self.status = "Microphone off".into();
                } else {
                    self.mode = Mode::Voice;
                    self.status = "Hold the button to talk".into();
                }
                Task::none()
            }
            Message::StartRecording => {
                if self.mode != Mode::Voice || self.voice_busy || self.recording.is_some() {
                    return Task::none();
                }
                let config = VoiceConfig::from_env();
                match Recording::start() {
                    Ok(recording) => {
                        self.voice_config = Some(config);
                        self.recording = Some(recording);
                        self.status = "Listening — release to send".into();
                    }
                    Err(error) => {
                        self.status = error.to_string();
                        tracing::warn!(%error, "could not start push-to-talk recording");
                    }
                }
                Task::none()
            }
            Message::StopRecording => self.finish_recording(),
            Message::VoiceComplete(result) => match result {
                Ok(reply) => {
                    self.history
                        .push((reply.transcript.clone(), reply.answer.clone()));
                    if self.history.len() > 8 {
                        self.history.remove(0);
                    }
                    self.transcript = Some(reply.transcript);
                    self.answer = Some(reply.answer.clone());
                    if let Some(piper_model) = reply.piper_model {
                        self.status = "Reply ready; speaking…".into();
                        let answer = reply.answer;
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || speak(&piper_model, &answer))
                                    .await
                                    .map_err(|error| error.to_string())?
                                    .map_err(|error| error.to_string())
                            },
                            |result| Message::SpeechComplete(result).into(),
                        )
                    } else {
                        self.voice_busy = false;
                        self.status = "Reply ready".into();
                        Task::none()
                    }
                }
                Err(error) => {
                    self.voice_busy = false;
                    self.status = format!("Voice request failed: {error}");
                    tracing::warn!(%error, "voice request failed");
                    Task::none()
                }
            },
            Message::SpeechComplete(result) => {
                self.voice_busy = false;
                self.status = match result {
                    Ok(()) => "Reply ready".into(),
                    Err(error) => {
                        tracing::warn!(%error, "speech output failed");
                        format!("Reply ready; speech output failed: {error}")
                    }
                };
                Task::none()
            }
            Message::NetworkStatus(status) => {
                let Some(reaction) = self.policy.network_status_changed(status) else {
                    return Task::none();
                };
                let old_size = self.drawer.size();
                self.drawer.update(Input::Network(reaction), Instant::now());
                let size = self.drawer.size();
                if old_size != size {
                    set_size(self.surface, Some(size.0), Some(size.1))
                } else {
                    Task::none()
                }
            }
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
                    if !inside && self.recording.is_some() {
                        return self.finish_recording();
                    }
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
        let network = if self.policy.mode() == Mode::Off {
            Subscription::none()
        } else {
            Subscription::run_with(std::any::TypeId::of::<NetworkWatcher>(), |_| {
                iced::stream::channel::<Message>(
                    8,
                    |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
                        tokio::spawn(async move {
                            if let Err(error) = character_watch::watch_network_status(sender).await
                            {
                                tracing::warn!(%error, "NetworkManager monitoring is unavailable");
                            }
                        });

                        while let Some(status) = receiver.recv().await {
                            if output.send(Message::NetworkStatus(status)).await.is_err() {
                                break;
                            }
                        }
                    },
                )
            })
        };
        Subscription::batch([pointer, timeout, network])
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
            let microphone_status = if self.recording.is_some() {
                "Microphone active"
            } else if self.mode == Mode::Voice {
                "Voice enabled"
            } else {
                "Microphone off"
            };
            let voice_toggle = button::standard(if self.mode == Mode::Voice {
                "Микрофон: вкл."
            } else {
                "Микрофон: выкл."
            })
            .on_press_maybe(
                (!self.voice_busy && self.recording.is_none()).then_some(Message::ToggleVoice),
            );
            let recording = self.recording.is_some();
            let microphone_button = widget::MouseArea::new(
                container(text::body("🎙").class(cosmic::theme::Text::Color(Color::WHITE)))
                    .width(Length::Fixed(44.0))
                    .height(Length::Fixed(44.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(
                            if recording {
                                Color::from_rgb8(26, 176, 113)
                            } else {
                                Color::from_rgb8(60, 70, 88)
                            }
                            .into(),
                        ),
                        border: Border {
                            color: if recording {
                                Color::from_rgb8(78, 238, 164)
                            } else {
                                Color::from_rgb8(99, 112, 137)
                            },
                            width: 1.0,
                            radius: 22.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .on_press(Message::StartRecording)
            .on_release(Message::StopRecording)
            .on_exit(Message::StopRecording);
            let header = row([])
                .push(text::heading("Fen"))
                .push(text::caption("ДЕМО-РЕЖИМ").class(cosmic::theme::Text::Color(accent)))
                .push(widget::Space::new().width(Length::Fill))
                .push(voice_toggle)
                .push(microphone_button)
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
                            "Выберите событие или дождитесь изменения состояния сети.",
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
            let demo_controls =
                ManualEvent::ALL
                    .into_iter()
                    .fold(row([]).spacing(10), |row, event| {
                        row.push(
                            button::standard(event.label())
                                .on_press(Message::Input(Input::Manual(event)))
                                .width(Length::Fill),
                        )
                    });

            let mut dialog = column([]).spacing(4);
            if let Some(transcript) = &self.transcript {
                dialog = dialog
                    .push(text::caption("You"))
                    .push(text::body(transcript.clone()));
            }
            if let Some(answer) = &self.answer {
                dialog = dialog
                    .push(text::caption("Fen"))
                    .push(text::body(answer.clone()));
            }

            column([])
                .push(header)
                .push(portrait)
                .push(
                    text::caption("ДЕМО-СОБЫТИЯ")
                        .class(cosmic::theme::Text::Color(Color::from_rgb8(142, 160, 185))),
                )
                .push(demo_controls)
                .push(
                    row([])
                        .push(text::caption(microphone_status))
                        .push(widget::Space::new().width(Length::Fill))
                        .push(text::caption(self.status.clone()))
                        .spacing(8),
                )
                .push(dialog)
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

    #[test]
    fn voice_mode_requires_an_explicit_toggle_and_starts_with_microphone_off() {
        let (mut app, _) = Character::init(Core::default(), ());
        assert_eq!(app.mode, Mode::EventsOnly);
        assert!(app.recording.is_none());

        let _ = app.update(Message::StartRecording);
        assert!(app.recording.is_none());

        let _ = app.update(Message::ToggleVoice);
        assert_eq!(app.mode, Mode::Voice);
        assert!(app.recording.is_none());

        let _ = app.update(Message::ToggleVoice);
        assert_eq!(app.mode, Mode::EventsOnly);
        assert!(app.recording.is_none());
    }
}
