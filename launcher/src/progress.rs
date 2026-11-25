//! What the launcher is doing, shared between the thread doing the work and the
//! window showing it. When no window could be opened it prints to the terminal
//! instead, as the launcher did before it had a window.

use std::io::Write;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// A button the player can answer a question with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    TryAgain,
    Close,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::TryAgain => "Try again",
            Choice::Close => "Close",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileState {
    Waiting,
    Downloading,
    Done,
}

/// One file of the current download.
pub struct FileRow {
    pub name: String,
    /// Unknown until the download starts, if the server did not say beforehand.
    pub size: Option<u64>,
    /// Bytes downloaded so far.
    pub done: u64,
    pub state: FileState,
}

/// Something the worker is waiting for the player to answer.
pub struct Question {
    pub message: String,
    pub choices: Vec<Choice>,
}

#[derive(Default)]
pub struct State {
    /// What is happening now, e.g. "Checking for updates…".
    pub headline: String,
    /// The installed build (none on a first install) and the one being installed.
    pub builds: Option<(Option<String>, String)>,
    /// The files being downloaded, in download order.
    pub files: Vec<FileRow>,
    pub warning: Option<String>,
    pub question: Option<Question>,
    answer: Option<Choice>,
}

impl State {
    /// Bytes downloaded and bytes to download in all, over the files whose size is known.
    pub fn totals(&self) -> (u64, u64) {
        self.files.iter().fold((0, 0), |(done, total), file| {
            let size = file.size.unwrap_or(file.done);
            let done_here = if file.state == FileState::Done { size } else { file.done.min(size) };
            (done + done_here, total + size)
        })
    }

    /// The file being downloaded, with its index.
    pub fn current(&self) -> Option<(usize, &FileRow)> {
        self.files.iter().enumerate().find(|(_, file)| file.state == FileState::Downloading)
    }
}

struct Shared {
    state: Mutex<State>,
    answered: Condvar,
}

/// The worker's handle on the window (or the terminal). Cheap to clone.
#[derive(Clone)]
pub struct Progress {
    shared: Arc<Shared>,
    /// The window's egui context, or `None` in the terminal.
    window: Option<eframe::egui::Context>,
}

impl Progress {
    pub fn terminal() -> Self {
        Self::new(None)
    }

    pub fn window(ctx: eframe::egui::Context) -> Self {
        Self::new(Some(ctx))
    }

    fn new(window: Option<eframe::egui::Context>) -> Self {
        let shared = Shared { state: Mutex::new(State::default()), answered: Condvar::new() };
        Self { shared: Arc::new(shared), window }
    }

    pub fn in_window(&self) -> bool {
        self.window.is_some()
    }

    /// The state, for the window to draw.
    pub fn state(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves nothing half-done worth refusing.
        self.shared.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn change(&self, f: impl FnOnce(&mut State)) {
        f(&mut self.state());
        if let Some(ctx) = &self.window {
            ctx.request_repaint();
        }
    }

    /// Clears everything, before trying again.
    pub fn reset(&self) {
        self.change(|state| *state = State::default());
    }

    /// Empty clears it.
    pub fn headline(&self, text: &str) {
        if !self.in_window() && !text.is_empty() {
            println!("{text}");
        }
        self.change(|state| state.headline = text.to_string());
    }

    /// Which build is being replaced (if any) by which.
    pub fn builds(&self, from: Option<&str>, to: &str) {
        if !self.in_window() {
            match from {
                Some(from) => println!("Updating {from} -> {to}"),
                None => println!("Installing {to}"),
            }
        }
        self.change(|state| state.builds = Some((from.map(str::to_string), to.to_string())));
    }

    /// The files about to be downloaded, with their sizes where known.
    pub fn plan_files(&self, files: Vec<(String, Option<u64>)>) {
        if !self.in_window() && files.len() > 1 {
            let known: u64 = files.iter().filter_map(|(_, size)| *size).sum();
            println!("Downloading {} files, {:.1} MB", files.len(), mb(known));
        }
        self.change(|state| {
            state.files = files
                .into_iter()
                .map(|(name, size)| FileRow { name, size, done: 0, state: FileState::Waiting })
                .collect();
        });
    }

    pub fn file_started(&self, index: usize) {
        self.change(|state| {
            if let Some(file) = state.files.get_mut(index) {
                if self.window.is_none() {
                    println!("  {}", file.name);
                }
                file.state = FileState::Downloading;
                file.done = 0;
            }
        });
    }

    /// `done` bytes of file `index` are in, of `size` if the server said.
    pub fn file_progress(&self, index: usize, done: u64, size: Option<u64>) {
        self.change(|state| {
            if let Some(file) = state.files.get_mut(index) {
                file.done = done;
                file.size = size.or(file.size);
                if self.window.is_none() {
                    print_progress(done, file.size);
                }
            }
        });
    }

    pub fn file_done(&self, index: usize) {
        if !self.in_window() {
            println!();
        }
        self.change(|state| {
            if let Some(file) = state.files.get_mut(index) {
                file.state = FileState::Done;
            }
        });
    }

    /// Shows a problem the launcher carries on after.
    pub fn warn(&self, text: &str) {
        if !self.in_window() {
            println!("{text}");
        }
        self.change(|state| state.warning = Some(text.to_string()));
    }

    /// Shows `message` with a button per choice, and waits for one to be
    /// clicked. In the terminal pressing Enter picks `Close` if it is offered,
    /// otherwise the first choice.
    pub fn ask(&self, message: &str, choices: &[Choice]) -> Choice {
        if !self.in_window() {
            let choice = if choices.contains(&Choice::Close) { Choice::Close } else { choices[0] };
            println!("\n{message}");
            match choice {
                Choice::Close => println!("\nPress Enter to close."),
                Choice::TryAgain => println!("Press Enter to try again."),
            }
            let _ = std::io::stdin().read_line(&mut String::new());
            return choice;
        }
        self.change(|state| {
            state.answer = None;
            state.question = Some(Question { message: message.to_string(), choices: choices.to_vec() });
        });
        let mut state = self.state();
        loop {
            if let Some(answer) = state.answer.take() {
                state.question = None;
                return answer;
            }
            state = self.shared.answered.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Answers the question on screen. Called by the window.
    pub fn answer(&self, choice: Choice) {
        self.state().answer = Some(choice);
        self.shared.answered.notify_all();
    }

    /// Closes the window, which ends the launcher.
    pub fn close(&self) {
        if let Some(ctx) = &self.window {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            ctx.request_repaint();
        }
    }
}

pub fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn print_progress(done: u64, total: Option<u64>) {
    match total {
        Some(total) if total > 0 => print!(
            "\r  {:.1} / {:.1} MB ({}%)   ",
            mb(done),
            mb(total),
            done * 100 / total
        ),
        _ => print!("\r  {:.1} MB   ", mb(done)),
    }
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_up_progress() {
        let progress = Progress::window(eframe::egui::Context::default());
        progress.plan_files(vec![("a".into(), Some(100)), ("b".into(), None), ("c".into(), Some(50))]);
        assert_eq!(progress.state().totals(), (0, 150));
        progress.file_started(0);
        progress.file_progress(0, 40, Some(100));
        assert_eq!(progress.state().totals(), (40, 150));
        assert_eq!(progress.state().current().map(|(i, _)| i), Some(0));
        progress.file_done(0);
        // The size of "b" becomes known when its download starts.
        progress.file_started(1);
        progress.file_progress(1, 10, Some(30));
        assert_eq!(progress.state().totals(), (110, 180));
        assert_eq!(progress.state().current().map(|(i, file)| (i, file.name.as_str())), Some((1, "b")));
    }

    #[test]
    fn waits_for_an_answer() {
        let progress = Progress::window(eframe::egui::Context::default());
        let window = progress.clone();
        let answerer = std::thread::spawn(move || {
            while window.state().question.is_none() {
                std::thread::yield_now();
            }
            window.answer(Choice::TryAgain);
        });
        assert_eq!(progress.ask("Broken", &[Choice::TryAgain, Choice::Close]), Choice::TryAgain);
        assert!(progress.state().question.is_none());
        answerer.join().unwrap();
    }
}
