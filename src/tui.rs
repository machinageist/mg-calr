use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, Write};

use crate::application::TodoQueryProjection;

/// Keys understood by the bounded calendar shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Refresh,
    Quit,
    Unknown,
}

impl Key {
    /// Parse a line-oriented key token. Raw terminal input is deliberately left
    /// to a later slice so this shell remains portable and testable.
    #[must_use]
    pub fn parse(input: &str) -> Self {
        match input.trim() {
            "k" | "up" | "\u{1b}[A" => Self::Up,
            "j" | "down" | "\u{1b}[B" => Self::Down,
            "r" | "refresh" => Self::Refresh,
            "q" | "quit" | "\u{1b}" => Self::Quit,
            _ => Self::Unknown,
        }
    }
}

/// Renderer-neutral state for the keyboard-first shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiState {
    selected: usize,
    quit: bool,
    refresh_requested: bool,
    status: String,
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiState {
    /// Create an initial shell state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            selected: 0,
            quit: false,
            refresh_requested: false,
            status: "ready".to_owned(),
        }
    }

    /// Apply one key without consulting persistence.
    pub fn apply(&mut self, key: Key, item_count: usize) {
        match key {
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => {
                if item_count > 0 {
                    self.selected = (self.selected + 1).min(item_count - 1);
                }
            }
            Key::Refresh => {
                self.refresh_requested = true;
                "refresh requested".clone_into(&mut self.status);
            }
            Key::Quit => self.quit = true,
            Key::Unknown => "unknown key (use j/k, r, or q)".clone_into(&mut self.status),
        }
    }

    /// Complete a refresh after persistence has supplied a new snapshot.
    pub fn complete_refresh(&mut self, item_count: usize) {
        self.selected = self.selected.min(item_count.saturating_sub(1));
        "refreshed".clone_into(&mut self.status);
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    #[must_use]
    pub const fn take_refresh_request(&mut self) -> bool {
        let requested = self.refresh_requested;
        self.refresh_requested = false;
        requested
    }

    /// Render the complete frame using stable ordering supplied by the query boundary.
    #[must_use]
    pub fn render(&self, todos: &[TodoQueryProjection]) -> String {
        let mut frame = String::new();
        frame.push_str("mg-calr | todos\n");
        frame.push_str("────────────────────────────────────────\n");
        if todos.is_empty() {
            frame.push_str("  (no todos)\n");
        } else {
            for (index, todo) in todos.iter().enumerate() {
                let marker = if index == self.selected { ">" } else { " " };
                let state = if todo.completed_at.is_some() {
                    "done"
                } else if todo.trashed_at.is_some() {
                    "trash"
                } else {
                    "open"
                };
                let _ = writeln!(
                    frame,
                    "{marker} [{state}] {} [{}]",
                    todo.title, todo.priority
                );
            }
        }
        frame.push_str("────────────────────────────────────────\n");
        let _ = writeln!(frame, "j/k move  r refresh  q quit | {}", self.status);
        frame
    }

    /// Run the line-oriented shell over a loaded todo snapshot.
    ///
    /// The callback is invoked only after `r`, allowing the command layer to
    /// keep all persistence behind the existing application boundary.
    ///
    /// # Errors
    /// Returns input or terminal output errors.
    pub fn run<R, F>(
        &mut self,
        todos: &mut Vec<TodoQueryProjection>,
        input: R,
        mut refresh: F,
    ) -> io::Result<()>
    where
        R: BufRead,
        F: FnMut() -> io::Result<Vec<TodoQueryProjection>>,
    {
        let mut output = io::stdout().lock();
        writeln!(output, "{}", self.render(todos))?;
        for line in input.lines() {
            let key = Key::parse(&line?);
            self.apply(key, todos.len());
            if self.take_refresh_request() {
                *todos = refresh()?;
                self.complete_refresh(todos.len());
            }
            if self.should_quit() {
                break;
            }
            writeln!(output, "{}", self.render(todos))?;
        }
        output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, TuiState};

    #[test]
    fn navigation_is_bounded_and_refresh_is_explicit() {
        let mut state = TuiState::new();
        state.apply(Key::Down, 2);
        state.apply(Key::Down, 2);
        assert_eq!(state.selected(), 1);
        state.apply(Key::Up, 2);
        assert_eq!(state.selected(), 0);
        state.apply(Key::Refresh, 2);
        assert!(state.take_refresh_request());
        assert!(!state.take_refresh_request());
    }

    #[test]
    fn quit_and_unknown_keys_have_deterministic_transitions() {
        let mut state = TuiState::new();
        state.apply(Key::Unknown, 0);
        assert_eq!(state.status, "unknown key (use j/k, r, or q)");
        state.apply(Key::Quit, 0);
        assert!(state.should_quit());
    }

    #[test]
    fn empty_render_is_stable() {
        let frame = TuiState::new().render(&[]);
        assert_eq!(
            frame,
            "mg-calr | todos\n────────────────────────────────────────\n  (no todos)\n────────────────────────────────────────\nj/k move  r refresh  q quit | ready\n"
        );
    }

    #[test]
    fn key_parser_accepts_keyboard_aliases() {
        assert_eq!(Key::parse("j"), Key::Down);
        assert_eq!(Key::parse("k"), Key::Up);
        assert_eq!(Key::parse("r"), Key::Refresh);
        assert_eq!(Key::parse("q"), Key::Quit);
    }
}
