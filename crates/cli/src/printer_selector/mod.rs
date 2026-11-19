use anyhow::{Result, anyhow};
use crossterm::{
    cursor::{Hide, MoveUp, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Attribute, Color, Print, SetAttribute, SetForegroundColor, Stylize},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use tokio::{sync::mpsc, time::sleep};

use crate::search::{FoundPrinter, start_search};
use std::{
    cmp::min,
    io::{self, Write},
    time::{Duration, Instant},
};

const LOADER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub async fn run_selector(alt_scan: bool, port: u16) -> Result<FoundPrinter> {
    let (tx, mut rx) = mpsc::unbounded_channel::<FoundPrinter>();
    let (sender_handle, receiver_handle) = start_search(alt_scan, port, tx).await?;

    let mut found: Vec<FoundPrinter> = Vec::new();
    let mut selected: usize = 0;
    let mut stdout = io::stdout();
    let mut loader_index = 0;

    enable_raw_mode()?;

    // Consume keyboard query
    sleep(Duration::from_millis(100)).await;
    while event::poll(Duration::ZERO)? {
        let _ = event::read()?;
    }

    execute!(stdout, Hide, Print("\r\n"))?;

    let mut written_lines = 0;
    let mut draw =
        |stdout: &mut io::Stdout, found: &[FoundPrinter], selected: usize| -> Result<()> {
            execute!(
                stdout,
                MoveUp(written_lines),
                Clear(ClearType::FromCursorDown)
            )?;

            execute!(
                stdout,
                SetForegroundColor(Color::Yellow),
                Print("?"),
                SetAttribute(Attribute::Reset),
                SetAttribute(Attribute::Bold),
                Print(" Select printer"),
                SetAttribute(Attribute::Reset),
                SetForegroundColor(Color::Grey),
                Print(" ›\r\n"),
                SetAttribute(Attribute::Reset),
            )?;

            let max_printer_name = found
                .iter()
                .map(|p| p.name.as_deref().unwrap_or_default().len())
                .max()
                .unwrap_or(0);

            for (i, printer) in found.iter().enumerate() {
                let name = printer.name.as_deref().unwrap_or("<unknown>");
                let ver = printer.ver.as_deref().unwrap_or("<unknown>");

                let marker = if i == selected { "❯" } else { " " };
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    Print(marker),
                    SetAttribute(Attribute::Reset)
                )?;

                if i == selected {
                    execute!(
                        stdout,
                        SetAttribute(Attribute::Bold),
                        SetForegroundColor(Color::Cyan)
                    )?;
                }

                let width = max_printer_name + 15;
                execute!(
                    stdout,
                    Print(format!(
                        " {:<width$} ({})\r\n",
                        format!("\"{name}\", ver {ver}"),
                        printer.ip
                    )),
                    SetAttribute(Attribute::Reset),
                )?;
            }

            loader_index = (loader_index + 1) % LOADER.len();
            execute!(
                stdout,
                SetAttribute(Attribute::Bold),
                Print(format!(
                    "\r\n{} Searching for other printers...\r\n",
                    LOADER[loader_index].green()
                )),
                SetAttribute(Attribute::Reset),
            )?;

            written_lines = found.len() as u16 + 3;
            Ok(())
        };

    draw(&mut stdout, &found, selected)?;

    loop {
        tokio::select! {
            Some(printer) = rx.recv() => {
                if let Some(p) = found.iter_mut().find(|p| p.hw_hash == printer.hw_hash) {
                    p._last_updated = Instant::now();
                } else {
                    found.push(printer);
                }
            }

            _ = sleep(Duration::from_millis(80)) => {
                found.retain(|p| p._last_updated.elapsed() < Duration::from_secs(10));
                // If current selected is not retained
                selected = min(selected, found.len().saturating_sub(1));

                if event::poll(Duration::ZERO)?
                    && let Event::Key(key) = event::read()? {

                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL) {
                        disable_raw_mode()?;

                        execute!(
                            stdout,
                            Show,
                        )?;

                        return Err(anyhow!(
                            "Printer selection cancelled"
                        ));
                    }

                    match key.code {
                        KeyCode::Up => {
                            if !found.is_empty() {
                                selected =
                                    selected.saturating_sub(1);
                            }
                        }

                        KeyCode::Down => {
                            if !found.is_empty() {
                                selected = (selected + 1)
                                    .min(found.len() - 1);
                            }
                        }

                        KeyCode::Enter => {
                            if !found.is_empty() {
                                break;
                            }
                        }

                        KeyCode::Esc => {
                            disable_raw_mode()?;

                            execute!(
                                stdout,
                                Show,
                            )?;

                            return Err(anyhow!(
                                "Printer selection cancelled"
                            ));
                        }

                        _ => {}
                    }
                }
                draw(
                    &mut stdout,
                    &found,
                    selected,
                )?;
            }
        }
    }

    let selected_printer = found[selected].clone();
    if !found.is_empty() {
        execute!(
            stdout,
            MoveUp(written_lines),
            Clear(ClearType::FromCursorDown)
        )?;

        let name = selected_printer.name.as_deref().unwrap_or("<unknown>");
        let ver = selected_printer.ver.as_deref().unwrap_or("<unknown>");

        execute!(
            stdout,
            SetForegroundColor(Color::DarkGreen),
            Print("✔"),
            SetAttribute(Attribute::Reset),
            SetAttribute(Attribute::Bold),
            Print("  Selected · "),
            SetForegroundColor(Color::DarkGreen),
            Print(format!(
                "\"{}\", ver {} ({})\r\n",
                name, ver, selected_printer.ip
            )),
            SetAttribute(Attribute::Reset),
        )?;
    }

    disable_raw_mode()?;
    execute!(stdout, Show)?;
    stdout.flush()?;

    sender_handle.abort();
    receiver_handle.abort();

    if found.is_empty() {
        return Err(anyhow!(
            "Printers in this local network not found. \
             Specify IP `--host <ip>`"
        ));
    }
    Ok(selected_printer)
}
