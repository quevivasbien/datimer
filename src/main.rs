use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::style::{Color, Print, SetForegroundColor, Stylize};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear};
use tokio::sync::mpsc;
use tokio::time;

const TIME_X: u16 = 14;

fn format_time(t: time::Duration) -> String {
    let secs = t.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn get_local_time() -> String {
    let now = chrono::Local::now();
    format!("{}", now.format("%H:%M:%S"))
}

#[tokio::main]
async fn main() {
    if enable_raw_mode().is_err() {
        return;
    }
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        cursor::Hide,
        Clear(crossterm::terminal::ClearType::All)
    )
    .unwrap();

    execute!(
        stdout,
        cursor::MoveTo(0, 0),
        Print("DATIMER".bold()),
        cursor::MoveTo(0, 1),
        Print("Press 'p' to pause, 'q' to quit")
    )
    .unwrap();
    let mut active_line = 3;

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
    execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        cursor::MoveTo(0, active_line),
        Print("Start time: "),
        cursor::MoveTo(TIME_X, active_line),
        Print(get_local_time())
    )
    .unwrap();
    active_line += 1;

    // Every 250ms, print the elapsed time
    // If there is a key press, exit the loop
    loop {
        if !paused {
            let elapsed = start.elapsed();
            execute!(
                stdout,
                SetForegroundColor(Color::Reset),
                cursor::MoveTo(0, active_line),
                Print("Elapsed: "),
                cursor::MoveTo(TIME_X, active_line),
                Print(format_time(running_total + elapsed).bold())
            )
            .unwrap();
        }
        if let Ok(c) = rx.try_recv() {
            match c {
                'q' => break,
                'p' | ' ' => {
                    if paused {
                        start = time::Instant::now();
                        execute!(
                            stdout,
                            SetForegroundColor(Color::Green),
                            cursor::MoveTo(0, active_line),
                            Print("Resumed at: "),
                            cursor::MoveTo(TIME_X, active_line),
                            Print(get_local_time())
                        )
                        .unwrap();
                        active_line += 1;
                        execute!(
                            stdout,
                            SetForegroundColor(Color::Reset),
                            cursor::MoveTo(0, active_line),
                            Print("Elapsed: "),
                            cursor::MoveTo(TIME_X, active_line),
                            Print(format_time(running_total).bold())
                        )
                        .unwrap();
                    } else {
                        running_total += start.elapsed();
                        execute!(
                            stdout,
                            SetForegroundColor(Color::Red),
                            cursor::MoveTo(0, active_line),
                            Print("Paused: "),
                            cursor::MoveTo(TIME_X, active_line),
                            Print(format_time(running_total))
                        )
                        .unwrap();
                        active_line += 1;
                    }
                    paused = !paused;
                }
                _ => (),
            }
        }
        time::sleep(time::Duration::from_millis(250)).await;
    }

    // Restore the terminal to its original state
    execute!(
        stdout,
        SetForegroundColor(Color::Reset),
        cursor::MoveTo(0, active_line + 2),
        cursor::Show
    )
    .unwrap();
    disable_raw_mode().unwrap();
}
