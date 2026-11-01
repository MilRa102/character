use character_types::Emotion;
use cosmic::{
    Renderer, Theme,
    iced::{Color, Point, Rectangle, Size, mouse},
    widget::canvas::{self, Frame, Geometry, Path, Stroke},
};

pub struct Mascot(pub Emotion);

pub fn accent(emotion: Emotion) -> Color {
    match emotion {
        Emotion::Happy | Emotion::Proud => Color::from_rgb8(98, 208, 214),
        Emotion::Curious | Emotion::Confused => Color::from_rgb8(167, 139, 250),
        Emotion::Worried => Color::from_rgb8(244, 193, 101),
        Emotion::Sleepy => Color::from_rgb8(132, 151, 180),
        Emotion::Alert => Color::from_rgb8(244, 120, 101),
    }
}

impl<Message> canvas::Program<Message, Theme, Renderer> for Mascot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.scale((bounds.width / 160.0).min(bounds.height / 144.0));
        let glow = accent(self.0);
        let outline = Stroke::default().with_color(glow).with_width(2.5);
        let body =
            Path::rounded_rectangle(Point::new(42.0, 91.0), Size::new(76.0, 50.0), 22.0.into());
        frame.fill(&body, Color::from_rgb8(44, 49, 66));
        frame.stroke(&body, outline);
        let head = Path::new(|p| {
            p.move_to(Point::new(30.0, 61.0));
            p.line_to(Point::new(24.0, 13.0));
            p.line_to(Point::new(59.0, 34.0));
            p.quadratic_curve_to(Point::new(80.0, 26.0), Point::new(101.0, 34.0));
            p.line_to(Point::new(136.0, 13.0));
            p.line_to(Point::new(130.0, 61.0));
            p.quadratic_curve_to(Point::new(132.0, 99.0), Point::new(80.0, 114.0));
            p.quadratic_curve_to(Point::new(28.0, 99.0), Point::new(30.0, 61.0));
            p.close();
        });
        frame.stroke(
            &head,
            Stroke::default()
                .with_color(Color { a: 0.12, ..glow })
                .with_width(11.0),
        );
        frame.fill(&head, Color::from_rgb8(36, 40, 55));
        frame.stroke(&head, outline);

        for (tip, inner, base) in [
            ((33.0, 28.0), (51.0, 40.0), (36.0, 54.0)),
            ((127.0, 28.0), (109.0, 40.0), (124.0, 54.0)),
        ] {
            let ear = Path::new(|p| {
                p.move_to(Point::from(tip));
                p.line_to(Point::from(inner));
                p.line_to(Point::from(base));
                p.close();
            });
            frame.fill(&ear, Color { a: 0.45, ..glow });
        }

        for x in [58.0, 102.0] {
            let eye = match self.0 {
                Emotion::Happy | Emotion::Proud => Path::new(|p| {
                    p.move_to(Point::new(x - 8.0, 76.0));
                    p.quadratic_curve_to(Point::new(x, 59.0), Point::new(x + 8.0, 76.0));
                }),
                Emotion::Sleepy => Path::line(Point::new(x - 8.0, 76.0), Point::new(x + 8.0, 78.0)),
                _ => Path::circle(
                    Point::new(x, 73.0),
                    if self.0 == Emotion::Curious && x > 80.0 {
                        8.0
                    } else {
                        6.0
                    },
                ),
            };
            if matches!(self.0, Emotion::Happy | Emotion::Proud | Emotion::Sleepy) {
                frame.stroke(&eye, outline.with_width(3.5));
            } else {
                frame.fill(&eye, glow);
                frame.fill(&Path::circle(Point::new(x + 1.5, 71.0), 2.0), Color::WHITE);
            }
        }

        if self.0 == Emotion::Worried {
            for (start, end) in [((49.0, 57.0), (65.0, 52.0)), ((95.0, 52.0), (111.0, 57.0))] {
                frame.stroke(&Path::line(Point::from(start), Point::from(end)), outline);
            }
        }
        let mouth = Path::new(|p| {
            p.move_to(Point::new(72.0, 94.0));
            p.quadratic_curve_to(
                Point::new(
                    80.0,
                    if self.0 == Emotion::Worried {
                        85.0
                    } else {
                        103.0
                    },
                ),
                Point::new(88.0, 94.0),
            );
        });
        frame.stroke(&mouth, outline);
        vec![frame.into_geometry()]
    }
}
