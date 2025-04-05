#![deny(clippy::unwrap_used)]

use std::collections::HashMap;

use actix_web::web::Data;
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Layout,
    style::{Color, Style, Stylize},
};
use serde::Serialize;

use crate::State;

#[derive(Serialize)]
struct TaskDurationTracing {
    name: String,
    ph: String,
    ts: f64,
    dur: f64,
    args: serde_json::Value,
    tid: usize,
}

fn get_task_row_index(
    start_time: &std::time::Instant,
    end_time: &std::time::Instant,
    end_by_row_index: &mut HashMap<usize, std::time::Instant>,
) -> usize {
    let mut row = 0;
    loop {
        let entry = end_by_row_index.entry(row).or_insert(*start_time);
        if *entry <= *start_time {
            *entry = *end_time;
            return row;
        }
        row += 1;
    }
}

pub fn run_tui(state: &Data<State>) -> Result<()> {
    let mut terminal = ratatui::init();

    let start_instant = std::time::Instant::now();

    loop {
        if *state.auto_scroll.lock() {
            state.tasks_table_state.lock().select_last();
        }
        {
            let state = state.clone();
            terminal
                .draw(|frame| {
                    draw_terminal(frame, state);
                })
                .expect("failed to draw terminal");
        }
        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            match crossterm::event::read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => {
                    break;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Up, ..
                }) => {
                    state.tasks_table_state.lock().select_previous();
                    *state.auto_scroll.lock() = false;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    ..
                }) => {
                    state.tasks_table_state.lock().select_next();
                    let is_at_end = state.tasks_table_state.lock().selected()
                        == Some(state.task_periods.tasks_num() - 1);
                    *state.auto_scroll.lock() = is_at_end;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Home,
                    ..
                }) => {
                    state.tasks_table_state.lock().select_first();
                    *state.auto_scroll.lock() = false;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::End, ..
                }) => {
                    state.tasks_table_state.lock().select_last();
                    *state.auto_scroll.lock() = true;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('s'),
                    ..
                }) => {
                    let save_path = state.data_dir.join("tasks.json");
                    let mut periods = state.task_periods.get_sorted_periods();
                    periods.sort_by_key(|p| p.start);

                    let mut end_by_row_index: HashMap<usize, std::time::Instant> = HashMap::new();

                    let mut tracing_data = vec![];
                    for period in periods {
                        let row_index = get_task_row_index(
                            &period.start,
                            &period.start.checked_add(period.duration).expect(""),
                            &mut end_by_row_index,
                        );

                        let mut args = serde_json::Map::new();
                        args.insert(
                            "name".into(),
                            serde_json::Value::String(period.name.clone()),
                        );

                        tracing_data.push(TaskDurationTracing {
                            name: period.category.clone(),
                            ph: "X".to_string(),
                            ts: period.start.duration_since(start_instant).as_secs_f64()
                                * 1_000_000f64,
                            dur: period.duration.as_secs_f64() * 1_000_000f64,
                            args: args.into(),
                            tid: row_index,
                        });
                    }
                    std::fs::write(save_path, serde_json::to_string_pretty(&tracing_data)?)?;
                }
                _ => {}
            }
        }
    }
    ratatui::restore();
    Ok(())
}

fn draw_terminal(frame: &mut ratatui::Frame, state: actix_web::web::Data<State>) {
    use ratatui::layout::Constraint::*;

    let tasks: Vec<crate::task_periods::TaskPeriod> = state.task_periods.get_sorted_periods();

    let mut tasks_table_state = state.tasks_table_state.lock();

    let vertical = Layout::vertical([Length(1), Min(0)]);
    let [title_area, main_area] = vertical.areas(frame.area());
    let text = ratatui::text::Text::raw(format!("ccelerate_server at http://{}", state.address));
    frame.render_widget(text, title_area);

    let success_style = Style::new().fg(Color::Green);
    let fail_style = Style::new().fg(Color::Red);
    let not_done_style = Style::new().fg(Color::Blue);

    let mut table = ratatui::widgets::Table::new(
        tasks.iter().map(|t| {
            ratatui::widgets::Row::new([
                ratatui::text::Text::raw(format!("{:3.1}s", t.duration.as_secs_f64())),
                ratatui::text::Text::raw(&t.category),
                ratatui::text::Text::raw(&t.name),
            ])
            .style(if t.active {
                not_done_style
            } else if t.finished_successfully {
                success_style
            } else {
                fail_style
            })
        }),
        [Length(10), Length(15), Percentage(100)],
    );
    if !*state.auto_scroll.lock() {
        table = table.row_highlight_style(Style::new().gray());
    }

    frame.render_stateful_widget(table, main_area, &mut tasks_table_state);
}
