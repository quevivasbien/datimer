use std::collections::VecDeque;
use std::fs::File;
use std::io::{Seek, Write};

use chrono::Timelike;
use crossterm::{cursor, terminal};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::style::{style, Color, Print, SetForegroundColor, StyledContent, Stylize};
use tokio::sync::mpsc;
use tokio::time;

const TIME_X: u16 = 14;
const REFRESH_MS: u64 = 128;
const SAVE_INTERVAL_S: u64 = 20;

struct Line {
    message: StyledContent<&'static str>,
    timestamp: StyledContent<String>,
    color: Color,
}

impl Line {
    fn new(message: &'static str, hms: (u8, u8, u8)) -> Line {
        let timestamp = format!("{:02}:{:02}:{:02}", hms.0, hms.1, hms.2);
        Self {
            message: style(message),
            timestamp: style(timestamp),
            color: Color::Reset,
        }
    }
    fn from_duration(message: &'static str, d: time::Duration) -> Line {
        let secs = d.as_secs();
        let hms = (
            (secs / 3600) as u8,
            ((secs % 3600) / 60) as u8,
            (secs % 60) as u8,
        );
        Self::new(message, hms)
    }
    fn from_datetime(message: &'static str, d: chrono::DateTime<chrono::Local>) -> Line {
        let hms = (
            d.hour() as u8,
            d.minute() as u8,
            d.second() as u8,
        );
        Self::new(message, hms)
    }
    fn color(mut self, color: Color) -> Line {
        self.color = color;
        self
    }
    fn bold(mut self) -> Line {
        self.message = self.message.bold();
        self.timestamp = self.timestamp.bold();
        self
    }
    fn italic(mut self) -> Line {
        self.message = self.message.italic();
        self.timestamp = self.timestamp.italic();
        self
    }

    fn print(&self, stdout: &mut std::io::Stdout, line: u16) {
        execute!(
            stdout,
            SetForegroundColor(self.color),
            cursor::MoveTo(0, line),
            Print(&self.message),
            cursor::MoveTo(TIME_X, line),
            Print(&self.timestamp)
        )
        .unwrap();
    }
}

struct History {
    lines: VecDeque<Line>,
    start_row: u16,
    max_rows: u16,
    history_file: File,
    last_save: time::Instant,
}

impl History {
    fn new(history_file: File) -> History {
        Self {
            lines: VecDeque::new(),
            start_row: 4,
            max_rows: terminal::size().unwrap().1 - 6,
            history_file,
            last_save: time::Instant::now(),
        }
    }

    fn len(&self) -> u16 {
        self.lines.len() as u16
    }
    fn active_line(&self) -> u16 {
        self.len() + self.start_row
    }

    fn update_history(&mut self) -> std::io::Result<()> {
        // Clear the history file
        self.history_file.seek(std::io::SeekFrom::Start(0))?;
        self.history_file.set_len(0)?;
        // Write the current history
        for line in &self.lines {
            writeln!(self.history_file, "{} {}", line.message.content(), line.timestamp.content())?;
        }
        Ok(())
    }

    fn write_line(&mut self, line: Line, stdout: &mut std::io::Stdout, advance: bool) -> std::io::Result<()> {
        // Update the history file, if needed
        let now = time::Instant::now();
        if advance || now - self.last_save >= time::Duration::from_secs(SAVE_INTERVAL_S) {
            self.last_save = now;
            self.update_history()?;
        }

        if !advance {
            // Replace the current line
            line.print(stdout, u16::max(self.start_row, self.active_line() - 1));
            self.lines.pop_back();
            self.lines.push_back(line);
            return Ok(());
        }

        // Add a new line
        if self.len() < self.max_rows {
            line.print(stdout, self.active_line());
            self.lines.push_back(line);
            return Ok(());
        }
        
        self.lines.pop_front();
        self.lines.push_back(line);
        
        // move all lines up
        execute!(
            stdout,
            cursor::MoveToRow(self.start_row),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        )?;
        for (i, line) in self.lines.iter().enumerate() {
            line.print(stdout, self.start_row + i as u16);
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = std::env::args();
    let save_file = match args.skip(1).next() {
        Some(filename) => File::create(filename),
        None => File::create(".datimer.history"),
    }?;

    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        cursor::Hide,
        terminal::Clear(crossterm::terminal::ClearType::All)
    )?;

    execute!(
        stdout,
        cursor::MoveTo(0, 0),
        Print("DATIMER".bold()),
        cursor::MoveTo(0, 1),
        Print("Press 'p' to pause, 'q' to quit")
    )?;

    // Create a channel for communication between tasks
    let (tx, mut rx) = mpsc::channel(1);

    // Spawn a task to listen for key presses
    tokio::spawn(async move {
        loop {
            if let Ok(Event::Key(KeyEvent { code, .. })) = event::read() {
                if let KeyCode::Char(c) = code {
                    if tx.send(c).await.is_err() {
                        return;
                    }
                    if c == 'q' {
                        return;
                    }
                }
            }
        }
    });

    let mut running_total = time::Duration::from_secs(0);
    let mut start = time::Instant::now();
    let mut paused = false;
    Line::from_datetime("Start time:", chrono::Local::now()).color(Color::Cyan).print(&mut stdout, 3);

    let mut history = History::new(save_file);

    // Every REFRESH_MS ms, print the elapsed time
    // If there is a key press, exit the loop
    loop {
        if !paused {
            let elapsed = start.elapsed();
            let line = Line::from_duration("Elapsed:", running_total + elapsed).color(Color::Reset).bold();
            history.write_line(line, &mut stdout, false)?;
        }
        if let Ok(c) = rx.try_recv() {
            match c {
                'q' => break,
                'p' | ' ' => {
                    if paused {
                        // resume
                        start = time::Instant::now();
                        let line = Line::from_datetime("Resumed at:", chrono::Local::now()).color(Color::Green);
                        history.write_line(line, &mut stdout, false)?;
                        let line = Line::from_duration("Elapsed:", running_total).color(Color::Reset).bold();
                        history.write_line(line, &mut stdout, true)?;
                    } else {
                        // pause
                        running_total += start.elapsed();
                        let line = Line::from_datetime("Paused at:", chrono::Local::now()).color(Color::Red);
                        history.write_line(line, &mut stdout, false)?;
                        let line = Line::from_duration("Elapsed:", running_total).color(Color::Reset).italic();
                        history.write_line(line, &mut stdout, true)?;
                    }
                    paused = !paused;
                }
                _ => (),
            }
        }
        time::sleep(time::Duration::from_millis(REFRESH_MS)).await;
    }

    // Restore the terminal to its original state
    execute!(
        stdout,
        SetForegroundColor(Color::Reset),
        cursor::MoveTo(0, history.active_line() + 2),
        cursor::Show
    )?;
    terminal::disable_raw_mode()?;

    Ok(())
}
