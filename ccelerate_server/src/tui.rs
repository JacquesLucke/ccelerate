#![deny(clippy::unwrap_used)]

use actix_web::web::Data;
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Layout,
    style::{Color, Style},
};

use crate::State;

pub fn run_tui(state: &Data<State>) -> Result<()> {
    let mut terminal = ratatui::init();

    loop {
        let state_clone = state.clone();
        state_clone.tasks_table_state.lock().select_last();
        terminal
            .draw(|frame| {
                draw_terminal(frame, state_clone);
            })
            .expect("failed to draw terminal");
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

    let table = ratatui::widgets::Table::new(
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

    frame.render_stateful_widget(table, main_area, &mut tasks_table_state);
}
