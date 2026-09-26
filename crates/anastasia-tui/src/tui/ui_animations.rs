use ratatui::prelude::*;
use std::sync::OnceLock;

// Original compact terminal illustrations, embedded in the binary.
const ART: &[&str] = &[
    r#"              . . .
         .-'         '-.
      .-'   .-------.   '-.
    <      /  .---.  \      >
      '-. |  ( @ )  | .-'
         '\  '---'  /'
           '-------'
              ' ' '"#,
    r#"          .---------.
       .-' .   |   . '-.
      / .  .---+---.  . \
     |----/----|----\----|
     | . |     |     | . |
     |----\----|----/----|
      \ .  '---+---'  . /
       '-. .   |   .-'
          '---------'"#,
    r#"       .------------------.
       |  >_              |
       |  [ ACCESS OK ]   |
       |  0101 0010 1010  |
       '------------------'
              |    |
          .---'----'---.
      ___/______________\___
     / [_][_][_][_][_][_]  /
    '---------------------'"#,
    r#"     o----+       +----o
          |  .-.  |
     +----+--|#|--+----+
     |       '-'       |
     +--o   [CPU]   o--+
        |     |     |
     o--+-----+-----+--o
              |
          o---+---o"#,
    r#"                 *
            .  .   .
         \           /
          \  .---.  /
           \(     )/
            '-----'
               /|
          ____/ |____
         /___________\"#,
    r#"       /\             /\
      /  \    _____  /  \
     /____\  /_____\/____\
       ||   | [] [] | ||
       ||   |  __  | ||
      _||___|_|  |_|_||_
          .  /__\  .
       .    /____\    .
          .        ."#,
    r#"       /\_/\
      ( o.o )   .-----------.
       > ^ <    | >_        |
      /|   |\   |  hello!   |
     (_|___|_)  '-----------'
        / \          | |
       /___\     ____|_|____
      (_____)   /___________/
         '---.________."#,
    r#"          .-------.
         /  /////  \
        |  [o] [o]  |
        |     ^     |
         \  '---'  /
       .--'-------'--.
      /  /|  [#]  |\  \
     /__/ |_______| \__\
          /  |  \
         /___|___\"#,
];
static LAUNCH_ART: OnceLock<usize> = OnceLock::new();

pub(crate) fn initialize_welcome_art() {
    LAUNCH_ART.get_or_init(|| rand::random_range(0..ART.len()));
}

fn selected_art() -> &'static str {
    // Rendering only reads the startup selection; tests default to the eye.
    ART[*LAUNCH_ART.get().unwrap_or(&0)]
}

fn fits(art: &str, width: u16, height: u16) -> bool {
    art.lines().count() + 1 <= usize::from(height)
        && art.lines().all(|line| line.len() <= usize::from(width))
}

pub(crate) fn welcome_title_height(show: bool, input_height: u16, width: u16, height: u16) -> u16 {
    if !show || height == 0 {
        return 0;
    }
    let art = selected_art();
    let preferred = (art.lines().count() as u16 + 1)
        .saturating_sub(input_height.saturating_sub(1))
        .min(height);
    if fits(art, width, preferred) {
        preferred
    } else {
        1
    }
}

pub(super) fn draw_welcome_title(frame: &mut Frame, area: Rect) {
    draw_art(frame, area, selected_art());
}

fn draw_art(frame: &mut Frame, area: Rect, art: &str) {
    if area.height == 0 {
        return;
    }
    let show_art = fits(art, area.width, area.height);
    let name_y = if show_art {
        let width = art.lines().map(str::len).max().unwrap_or(0) as u16;
        let x = area.x + area.width.saturating_sub(width) / 2;
        for (y, line) in art.lines().enumerate() {
            frame
                .buffer_mut()
                .set_string(x, area.y + y as u16, line, Style::default().dim());
        }
        area.y + art.lines().count() as u16
    } else {
        area.y
    };
    frame.buffer_mut().set_stringn(
        area.x + area.width.saturating_sub(9) / 2,
        name_y,
        "Anastasia",
        usize::from(area.width),
        Style::default().dim(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn bundled_art_dimensions_and_rendering() {
        assert_eq!(ART.len(), 8);
        for art in ART {
            assert!(art.is_ascii());
            assert!((8..=12).contains(&art.lines().count()));
            assert!(art.lines().all(|line| line.len() <= 48));
            let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
            terminal
                .draw(|frame| draw_art(frame, frame.area(), art))
                .unwrap();
            let wide = terminal.backend().buffer().clone();
            terminal
                .draw(|frame| draw_art(frame, frame.area(), art))
                .unwrap();
            assert_eq!(&wide, terminal.backend().buffer());
            let mut narrow = Terminal::new(TestBackend::new(12, 4)).unwrap();
            narrow
                .draw(|frame| draw_art(frame, frame.area(), art))
                .unwrap();
            assert_eq!(narrow.backend().buffer()[(1, 0)].symbol(), "A");
            assert!(
                narrow.backend().buffer().content[12..]
                    .iter()
                    .all(|cell| cell.symbol() == " ")
            );
        }
    }

    #[test]
    fn launch_selection_is_stable_and_composer_space_collapses() {
        initialize_welcome_art();
        let selected = selected_art();
        initialize_welcome_art();
        assert_eq!(selected, selected_art());
        assert_eq!(welcome_title_height(false, 1, 60, 20), 0);
        assert_eq!(welcome_title_height(true, 1, 60, 0), 0);
        assert_eq!(
            welcome_title_height(true, 1, 60, 20),
            selected.lines().count() as u16 + 1
        );
        assert_eq!(welcome_title_height(true, 2, 60, 20), 1);
        assert_eq!(welcome_title_height(true, 1, 12, 20), 1);
        assert_eq!(welcome_title_height(true, 1, 60, 4), 1);
    }
}
