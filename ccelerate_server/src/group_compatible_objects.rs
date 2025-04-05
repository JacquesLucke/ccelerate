use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use bstr::{BString, ByteVec};

use crate::{
    args_processing, gcc_args, state::State, state_persistent::ObjectData,
    task_periods::TaskPeriodInfo,
};

#[derive(Debug, Clone)]
pub struct CompileChunk {
    pub records: Vec<ObjectData>,
}

pub fn known_object_files_to_chunks(
    original_object_records: &[ObjectData],
    state: &Arc<State>,
) -> Result<Vec<CompileChunk>> {
    let task_period = state.task_periods.start(GroupObjectsToChunksTaskInfo {
        num: original_object_records.len(),
    });

    let mut chunks: HashMap<BString, CompileChunk> = HashMap::new();
    for record in original_object_records {
        let info = args_processing::BuildObjectFileInfo::from_args(
            record.create.binary,
            &record.create.cwd,
            &record.create.args,
        )?;

        let mut chunk_key = BString::new(Vec::new());
        chunk_key.push_str(
            record
                .create
                .binary
                .to_standard_binary_name()
                .as_encoded_bytes(),
        );
        chunk_key.push_str(info.source_language.to_valid_ext());
        gcc_args::add_translation_unit_unspecific_args_to_key(&record.create.args, &mut chunk_key)?;
        chunk_key.push_str(record.create.cwd.as_os_str().as_encoded_bytes());
        for include_define in &record.local_code.include_defines {
            chunk_key.push_str(include_define);
        }
        for bad_include in &record.local_code.bad_includes {
            chunk_key.push_str(bad_include.as_os_str().as_encoded_bytes());
        }
        let chunk = chunks.entry(chunk_key).or_insert_with(|| CompileChunk {
            records: Vec::new(),
        });
        chunk.records.push(record.clone());
    }
    task_period.finished_successfully();
    Ok(chunks.into_values().collect())
}

struct GroupObjectsToChunksTaskInfo {
    num: usize,
}

impl TaskPeriodInfo for GroupObjectsToChunksTaskInfo {
    fn category(&self) -> String {
        "Group Chunks".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        format!("Objects: {}", self.num)
    }

    fn log_detailed(&self) {
        log::info!("Group objects to chunks");
    }
}
