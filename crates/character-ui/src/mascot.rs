use std::sync::LazyLock;

use character_types::Emotion;
use cosmic::iced::widget::image::Handle;

static CALM: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/fen/fen_calm.png")[..]));
static NEW: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/fen/fen_new.png")[..]));
static DONE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/fen/fen_done.png")[..]));
static BATTERY: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/fen/fen_battery.png")[..]));
static REST: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/fen/fen_rest.png")[..]));

pub fn calm() -> Handle {
    CALM.clone()
}

pub fn image(emotion: Emotion) -> Handle {
    match emotion {
        Emotion::Happy | Emotion::Proud => DONE.clone(),
        Emotion::Curious | Emotion::Confused => NEW.clone(),
        Emotion::Worried => BATTERY.clone(),
        Emotion::Sleepy => REST.clone(),
        Emotion::Alert => CALM.clone(),
    }
}

pub fn accent(emotion: Emotion) -> cosmic::iced::Color {
    match emotion {
        Emotion::Happy | Emotion::Proud => cosmic::iced::Color::from_rgb8(74, 224, 222),
        Emotion::Curious | Emotion::Confused => cosmic::iced::Color::from_rgb8(153, 135, 255),
        Emotion::Worried => cosmic::iced::Color::from_rgb8(255, 164, 91),
        Emotion::Sleepy => cosmic::iced::Color::from_rgb8(130, 165, 205),
        Emotion::Alert => cosmic::iced::Color::from_rgb8(255, 91, 87),
    }
}
