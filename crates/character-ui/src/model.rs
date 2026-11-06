use std::time::{Duration, Instant};

use character_core::NetworkReaction;
use character_types::{Emotion, State};

pub const FOLD_AFTER: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualEvent {
    Message,
    Success,
    Rest,
}

impl ManualEvent {
    pub const ALL: [Self; 3] = [Self::Message, Self::Success, Self::Rest];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Message => "Новое",
            Self::Success => "Готово",
            Self::Rest => "Отдых",
        }
    }

    pub const fn reaction(self) -> (Emotion, Tooltip) {
        match self {
            Self::Message => (
                Emotion::Curious,
                Tooltip {
                    title: "Что-то новое",
                    body: "Пришло демо-сообщение. Посмотрим вместе?",
                },
            ),
            Self::Success => (
                Emotion::Happy,
                Tooltip {
                    title: "Готово!",
                    body: "Демо завершено — никаких действий в системе не выполнялось.",
                },
            ),
            Self::Rest => (
                Emotion::Sleepy,
                Tooltip {
                    title: "Тихая минутка",
                    body: "Немного отдыха? Я буду рядом, когда понадоблюсь.",
                },
            ),
        }
    }
}

pub const fn network_reaction(reaction: NetworkReaction) -> (Emotion, Tooltip) {
    match reaction {
        NetworkReaction::Disconnected => (
            Emotion::Worried,
            Tooltip {
                title: "Сеть отключена",
                body: "Соединение пропало. Подожду, пока оно восстановится.",
            },
        ),
        NetworkReaction::Connected => (
            Emotion::Happy,
            Tooltip {
                title: "Сеть восстановлена",
                body: "Соединение снова доступно.",
            },
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tooltip {
    pub title: &'static str,
    pub body: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum Input {
    Toggle,
    Manual(ManualEvent),
    Network(NetworkReaction),
    PointerEntered,
    PointerLeft,
    Tick(Instant),
}

#[derive(Debug)]
pub struct Drawer {
    pub state: State,
    pub emotion: Emotion,
    pub tooltip: Option<Tooltip>,
    pub deadline: Option<Instant>,
    pointer_inside: bool,
}

impl Default for Drawer {
    fn default() -> Self {
        Self {
            state: State::Peek,
            emotion: Emotion::Curious,
            tooltip: None,
            deadline: None,
            pointer_inside: false,
        }
    }
}

impl Drawer {
    pub fn expanded(&self) -> bool {
        self.state == State::Idle
    }

    pub fn size(&self) -> (u32, u32) {
        if self.expanded() {
            (520, 520)
        } else {
            (160, 40)
        }
    }

    pub fn update(&mut self, input: Input, now: Instant) {
        match input {
            Input::Toggle if self.expanded() => self.fold(),
            Input::Toggle => {
                self.state = State::Idle;
                self.arm_timeout(now);
            }
            Input::Manual(event) => {
                let (emotion, tooltip) = event.reaction();
                self.state = State::Idle;
                self.emotion = emotion;
                self.tooltip = Some(tooltip);
                self.arm_timeout(now);
            }
            Input::Network(reaction) => {
                let (emotion, tooltip) = network_reaction(reaction);
                self.state = State::Idle;
                self.emotion = emotion;
                self.tooltip = Some(tooltip);
                self.arm_timeout(now);
            }
            Input::PointerEntered => {
                self.pointer_inside = true;
                self.deadline = None;
            }
            Input::PointerLeft => {
                self.pointer_inside = false;
                self.arm_timeout(now);
            }
            Input::Tick(tick) => {
                if self.deadline.is_some_and(|deadline| tick >= deadline) {
                    self.fold();
                }
            }
        }
    }

    fn arm_timeout(&mut self, now: Instant) {
        self.deadline = (self.expanded() && !self.pointer_inside).then_some(now + FOLD_AFTER);
    }

    fn fold(&mut self) {
        self.state = State::Peek;
        self.tooltip = None;
        self.deadline = None;
        // The resized surface may no longer contain the pointer.
        self.pointer_inside = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_as_a_small_reopenable_tab() {
        let mut drawer = Drawer::default();
        assert_eq!(drawer.state, State::Peek);
        assert_eq!(drawer.size(), (160, 40));
        assert!(drawer.tooltip.is_none());
        assert!(drawer.deadline.is_none());
        drawer.update(Input::Toggle, Instant::now());
        assert!(drawer.expanded());
        assert_eq!(drawer.size(), (520, 520));
    }

    #[test]
    fn manual_events_open_the_drawer_and_replace_the_reaction() {
        let mut drawer = Drawer::default();
        let now = Instant::now();
        for event in ManualEvent::ALL {
            drawer.update(Input::Manual(event), now);
            let (emotion, tooltip) = event.reaction();
            assert!(drawer.expanded());
            assert_eq!(drawer.emotion, emotion);
            assert_eq!(drawer.tooltip, Some(tooltip));
            assert_eq!(drawer.deadline, Some(now + FOLD_AFTER));
        }
        let emotions: std::collections::HashSet<_> = ManualEvent::ALL
            .map(|event| event.reaction().0)
            .into_iter()
            .collect();
        assert_eq!(emotions.len(), 3);
    }

    #[test]
    fn network_reactions_open_the_drawer_with_connectivity_tooltips() {
        let mut drawer = Drawer::default();
        let now = Instant::now();

        drawer.update(Input::Network(NetworkReaction::Disconnected), now);
        assert!(drawer.expanded());
        assert_eq!(drawer.emotion, Emotion::Worried);
        assert_eq!(
            drawer.tooltip.map(|tooltip| tooltip.title),
            Some("Сеть отключена")
        );

        drawer.update(Input::Network(NetworkReaction::Connected), now);
        assert_eq!(drawer.emotion, Emotion::Happy);
        assert_eq!(
            drawer.tooltip.map(|tooltip| tooltip.title),
            Some("Сеть восстановлена")
        );
    }

    #[test]
    fn folds_at_the_exact_deadline_and_clears_the_tooltip() {
        let mut drawer = Drawer::default();
        let now = Instant::now();
        drawer.update(Input::Manual(ManualEvent::Message), now);
        drawer.update(Input::Tick(now + FOLD_AFTER - Duration::from_nanos(1)), now);
        assert!(drawer.expanded());
        drawer.update(Input::Tick(now + FOLD_AFTER), now);
        assert_eq!(drawer.state, State::Peek);
        assert!(drawer.tooltip.is_none());
        assert!(drawer.deadline.is_none());
        drawer.update(Input::Toggle, now + FOLD_AFTER);
        assert!(drawer.expanded());
        assert!(drawer.tooltip.is_none());
    }

    #[test]
    fn new_events_restart_the_timeout_and_stale_ticks_do_not_fold() {
        let mut drawer = Drawer::default();
        let now = Instant::now();
        drawer.update(Input::Manual(ManualEvent::Message), now);
        drawer.update(Input::Manual(ManualEvent::Success), now + FOLD_AFTER / 2);
        drawer.update(Input::Tick(now + FOLD_AFTER), now);
        assert!(drawer.expanded());
        drawer.update(Input::Tick(now + FOLD_AFTER * 2), now);
        assert!(!drawer.expanded());
    }

    #[test]
    fn hovering_keeps_the_drawer_open_and_leaving_restarts_the_timeout() {
        let mut drawer = Drawer::default();
        let now = Instant::now();
        drawer.update(Input::PointerEntered, now);
        drawer.update(Input::Toggle, now);
        drawer.update(Input::Manual(ManualEvent::Rest), now);
        drawer.update(Input::Tick(now + FOLD_AFTER * 2), now);
        assert!(drawer.expanded());
        assert!(drawer.deadline.is_none());
        let left = now + FOLD_AFTER * 3;
        drawer.update(Input::PointerLeft, left);
        assert_eq!(drawer.deadline, Some(left + FOLD_AFTER));
        drawer.update(Input::Tick(left + FOLD_AFTER), left);
        assert!(!drawer.expanded());
    }

    #[test]
    fn manual_fold_does_not_leave_a_timer_running() {
        let mut drawer = Drawer::default();
        let now = Instant::now();
        drawer.update(Input::Manual(ManualEvent::Message), now);
        drawer.update(Input::Toggle, now);
        drawer.update(Input::PointerLeft, now);
        assert!(!drawer.expanded());
        assert!(drawer.deadline.is_none());
        assert!(drawer.tooltip.is_none());
    }
}
