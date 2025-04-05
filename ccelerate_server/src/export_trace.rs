use std::{collections::HashMap, path::Path};

use serde::Serialize;

use crate::task_periods::TaskPeriods;

#[derive(Serialize)]
struct TaskDurationTracing {
    name: String,
    ph: String,
    ts: f64,
    dur: f64,
    args: serde_json::Value,
    tid: usize,
    cat: String,
}

use anyhow::Result;

pub async fn export(
    path: &Path,
    task_periods: &TaskPeriods,
    start_instant: std::time::Instant,
) -> Result<()> {
    let mut periods = task_periods.get_sorted_periods();
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

        let mut name = period.category.clone();
        if !period.finished_successfully {
            name.push_str(" (failed)");
        }

        tracing_data.push(TaskDurationTracing {
            name,
            ph: "X".to_string(),
            ts: period.start.duration_since(start_instant).as_secs_f64() * 1_000_000f64,
            dur: period.duration.as_secs_f64() * 1_000_000f64,
            args: args.into(),
            tid: row_index,
            cat: "".into(),
        });
    }
    let json_data = serde_json::to_string_pretty(&tracing_data)?;
    tokio::fs::write(path, json_data).await?;
    Ok(())
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
