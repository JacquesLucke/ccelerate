use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use bstr::{BString, ByteVec};

use crate::{
    args_processing, state::State, state_persistent::ObjectData, task_periods::TaskPeriodInfo,
};

#[derive(Debug, Clone)]
pub struct CompatibleObjects {
    pub objects: nunny::Vec<Arc<ObjectData>>,
}

pub fn group_compatible_objects(
    objects: &[Arc<ObjectData>],
    state: &Arc<State>,
) -> Result<Vec<CompatibleObjects>> {
    let task_period = state
        .task_periods
        .start(GroupObjectsToChunksTaskInfo { num: objects.len() });
    let mut chunks: HashMap<BString, CompatibleObjects> = HashMap::new();
    for object in objects {
        let key = create_object_compatibility_key(object)?;
        chunks
            .entry(key)
            .and_modify(|chunk| chunk.objects.push(object.clone()))
            .or_insert_with(|| CompatibleObjects {
                objects: nunny::Vec::of(object.clone()),
            });
    }
    task_period.finished_successfully();
    Ok(chunks.into_values().collect())
}

fn create_object_compatibility_key(object: &ObjectData) -> Result<BString> {
    let info = args_processing::BuildObjectFileInfo::from_args(
        object.create.binary,
        &object.create.cwd,
        &object.create.args,
    )?;

    let mut key = BString::new(Vec::new());
    key.push_str(
        object
            .create
            .binary
            .to_standard_binary_name()
            .as_encoded_bytes(),
    );
    key.push_str(info.source_language.valid_ext());
    key.push_str(object.create.cwd.as_os_str().as_encoded_bytes());
    for include_define in &object.local_code.include_defines {
        key.push_str(include_define);
    }
    args_processing::add_object_compatibility_args_to_key(
        object.create.binary,
        &object.create.args,
        &mut key,
    )?;
    Ok(key)
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
