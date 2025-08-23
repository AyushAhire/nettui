use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Row, Table};
use ratatui::Terminal;

use sysinfo::Networks;


#[derive(Clone, Debug)]
struct RowData {
    interface: String,
    rx_bps: f64,
    tx_bps: f64,
    packets_in: u64,
    packets_out: u64,
    errors_in: u64,
    errors_out: u64,
}

fn human_bps(bps: f64) -> String {
    if bps < 1.0 { return "--".to_string(); } // show --
    if bps < 1024.0 { return format!("{:.0} B/s", bps); } // just show bytes

    //units we support
    let units = ["KB/s", "MB/s", "GB/s"];
    let mut v = bps / 1024.0; //convert bytes -> KB
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }

    // formatting (1 decimal unless big enough)
    if v >=100.0 {
        format!("{:.0} {}", v, units[i])
    } else {
        format!("{:.1} {}", v, units[i])
    }
}

fn collect(networks: &mut Networks, interval_secs: f64, _show_virtual: bool) -> Vec<RowData> {
    //if interval is 0, convert to 1, as we will be divinding it, can't divide by zero
    let interval_secs = if interval_secs <=0.0 {1.0} else { interval_secs };

    //refresh network counters
    networks.refresh(true);

    let mut rows: Vec<RowData> = Vec::new();

    for(name, data) in networks.iter() {
        let is_virtual = name.starts_with("lo")
            || name.starts_with("veth")
            || name.starts_with("docker")
            || name.starts_with("br-")
            || name.starts_with("vmnet")
            || name.starts_with("virbr");

        if !is_virtual && is_virtual {
            continue;
        }

        //recieved/transmitted return bytes since last refresh
        let rx_bps = data.received() as f64 / interval_secs;
        let tx_bps = data.transmitted() as f64 / interval_secs;

        let row = RowData {
            interface: name.to_string(),
            rx_bps,
            tx_bps,
            packets_in: data.packets_received(),
            packets_out: data.packets_transmitted(),
            errors_in: data.errors_on_received(),
            errors_out: data.errors_on_transmitted(),
        };

        rows.push(row);
    }

    //sort by descending, highest traffic network appears first
    rows.sort_by(|a, b| {
        let a_total = a.rx_bps + a.tx_bps;
        let b_total = b.rx_bps + b.tx_bps;

        b_total.partial_cmp(&a_total).unwrap_or(std::cmp::Ordering::Equal)

    });

    rows

}

fn main() -> Result<(), io::Error> {

    //Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create networks once the new_with_refreshed_list seeds the list of interfaces
    let mut networks = sysinfo::Networks::new_with_refreshed_list();

    let mut refresh_ms: u64 = 500;
    let mut show_virtual = false;
    let mut last = Instant::now();


// collect snapshot used by the UI

    loop {
        let now = Instant::now();
        let elapsed = now.duration_since(last).as_secs_f64();
        if elapsed <= 0.0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
    let rows = collect(&mut networks, elapsed, show_virtual);


        //check for quit event
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        //refresh data
        networks.refresh(true);

        //Render
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(3)].as_ref())
                .split(f.size());

            //build table rows from network stats
            let title = format!(
                " Nettui - live (q:quit  +/-:rate  i:virtual)   refresh: {} ms   ifaces: {} ",
                refresh_ms,
                rows.len()
            );

            let header = Paragraph::new(Span::raw(title))
                .block(
                    Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
                );
            f.render_widget(header, chunks[0]);

            // table header
            let header_row = Row::new(vec!["IINTERFACE", "RX/s", "TX/s", "PKTS In", "PKTS Out", "Err In", "Err Out"])
                .style(Style::default().add_modifier(Modifier::BOLD));


            let table_rows = rows.iter().map(|r| {
                Row::new(vec![
                    r.interface.clone(),
                    human_bps(r.rx_bps),
                    human_bps(r.tx_bps),
                    r.packets_in.to_string(),
                    r.packets_out.to_string(),
                    r.errors_in.to_string(),
                    r.errors_out.to_string(),
                ])
            });

            let widths = [
                Constraint::Length(16),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(8),
            ];

            let table = Table::new(table_rows, widths)
                .header(header_row)
                .block(
                    Block::default()
                        .title(Span::from("Interfaces"))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                )
                .column_spacing(1);

        // render into the second chunk (chunks[0] is header)
        f.render_widget(table, chunks[1]);

        })?;
    }

    //restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())

}
