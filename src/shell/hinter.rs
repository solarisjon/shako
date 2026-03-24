use nu_ansi_term::{Color, Style};
use reedline::DefaultHinter;

pub fn create_hinter() -> DefaultHinter {
    DefaultHinter::default().with_style(Style::new().fg(Color::DarkGray))
}
