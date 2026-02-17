use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub hash: String,
    pub mode: Option<u32>,
}

impl FileMeta {
    pub fn new(hash: impl Into<String>, mode: Option<u32>) -> Self {
        Self {
            hash: hash.into(),
            mode,
        }
    }

    pub fn with_mode(mut self, mode: Option<u32>) -> Self {
        self.mode = mode;
        self
    }

    #[cfg(unix)]
    pub fn from_fs(path: &Path, hash: impl Into<String>) -> std::io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path)?;
        Ok(Self::new(hash, Some(meta.permissions().mode())))
    }

    #[cfg(not(unix))]
    pub fn from_fs(path: &Path, hash: impl Into<String>) -> std::io::Result<Self> {
        let _ = std::fs::metadata(path)?;
        Ok(Self::new(hash, None))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    Added { path: PathBuf, meta: FileMeta },
    Modified { path: PathBuf, meta: FileMeta },
    Removed { path: PathBuf },
    PermissionChanged { path: PathBuf, mode: Option<u32> },
}

impl FileChange {
    pub fn path(&self) -> &PathBuf {
        match self {
            FileChange::Added { path, .. }
            | FileChange::Modified { path, .. }
            | FileChange::Removed { path }
            | FileChange::PermissionChanged { path, .. } => path,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub id: String,
    pub changes: Vec<FileChange>,
}

impl Layer {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            changes: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    id: String,
    base: Option<Arc<Plan>>,
    layers: Vec<Layer>,
}

impl Plan {
    pub fn new(id: impl Into<String>, base: Option<Arc<Plan>>) -> Self {
        Self {
            id: id.into(),
            base,
            layers: Vec::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn base(&self) -> Option<&Arc<Plan>> {
        self.base.as_ref()
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn apply_layer(&mut self, layer: Layer) {
        self.layers.push(layer);
    }

    pub fn clone_plan(base: &Arc<Plan>, new_id: impl Into<String>) -> Self {
        Self {
            id: new_id.into(),
            base: Some(Arc::clone(base)),
            layers: Vec::new(),
        }
    }

    pub fn get_file_system_state(&self) -> BTreeMap<String, FileMeta> {
        let mut state = if let Some(base) = &self.base {
            base.get_file_system_state()
        } else {
            BTreeMap::new()
        };
        for layer in &self.layers {
            apply_layer(&mut state, layer);
        }
        state
    }

    pub fn merge_last_write(
        new_id: impl Into<String>,
        plan_a: &Plan,
        plan_b: &Plan,
    ) -> Result<Self, MergeError> {
        let common_base = shared_base(plan_a, plan_b)?;
        let mut merged = Plan::new(new_id, common_base);
        merged.layers.extend(plan_a.layers.clone());
        merged.layers.extend(plan_b.layers.clone());
        Ok(merged)
    }

    pub fn merge_three_way(
        new_id: impl Into<String>,
        plan_a: &Plan,
        plan_b: &Plan,
    ) -> Result<Self, MergeError> {
        let common_base = shared_base(plan_a, plan_b)?;
        let base_state = match &common_base {
            Some(base) => base.get_file_system_state(),
            None => BTreeMap::new(),
        };
        let a_state = plan_a.get_file_system_state();
        let b_state = plan_b.get_file_system_state();

        let mut paths = BTreeSet::new();
        paths.extend(base_state.keys().cloned());
        paths.extend(a_state.keys().cloned());
        paths.extend(b_state.keys().cloned());

        let mut merged_state = BTreeMap::new();
        let mut conflicts = Vec::new();

        for path in paths {
            let base_entry = base_state.get(&path).cloned();
            let mut a_entry = a_state.get(&path).cloned();
            let mut b_entry = b_state.get(&path).cloned();

            coalesce_from_base(&mut a_entry, &base_entry);
            coalesce_from_base(&mut b_entry, &base_entry);
            coalesce_between(&mut a_entry, &mut b_entry);

            let resolved = if a_entry == b_entry {
                a_entry
            } else if a_entry == base_entry {
                b_entry
            } else if b_entry == base_entry {
                a_entry
            } else {
                conflicts.push(MergeConflict {
                    path: path.clone(),
                    base: base_entry,
                    a: a_entry.clone(),
                    b: b_entry.clone(),
                });
                continue;
            };

            if let Some(meta) = resolved {
                merged_state.insert(path, meta);
            }
        }

        if !conflicts.is_empty() {
            return Err(MergeError::Conflicts(conflicts));
        }

        let mut layer = Layer::new("merge");
        for (path, base_meta) in &base_state {
            match merged_state.get(path) {
                None => layer.changes.push(FileChange::Removed {
                    path: PathBuf::from(path),
                }),
                Some(meta) if meta != base_meta => {
                    if meta.hash == base_meta.hash
                        && meta.mode != base_meta.mode
                        && meta.mode.is_some()
                    {
                        layer.changes.push(FileChange::PermissionChanged {
                            path: PathBuf::from(path),
                            mode: meta.mode,
                        });
                    } else {
                        layer.changes.push(FileChange::Modified {
                            path: PathBuf::from(path),
                            meta: meta.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        for (path, meta) in &merged_state {
            if !base_state.contains_key(path) {
                layer.changes.push(FileChange::Added {
                    path: PathBuf::from(path),
                    meta: meta.clone(),
                });
            }
        }

        let mut merged = Plan::new(new_id, common_base);
        if !layer.changes.is_empty() {
            merged.apply_layer(layer);
        }
        Ok(merged)
    }
}

fn apply_layer(state: &mut BTreeMap<String, FileMeta>, layer: &Layer) {
    for change in &layer.changes {
        apply_change(state, change);
    }
}

fn apply_change(state: &mut BTreeMap<String, FileMeta>, change: &FileChange) {
    match change {
        FileChange::Added { path, meta } | FileChange::Modified { path, meta } => {
            state.insert(path.to_str().expect("").to_string(), meta.clone());
        }
        FileChange::Removed { path } => {
            state.remove(path.to_str().expect(""));
        }
        FileChange::PermissionChanged { path, mode } => {
            if let Some(entry) = state.get_mut(path.file_name().expect("").to_str().expect("")) {
                if mode.is_some() {
                    entry.mode = *mode;
                }
            }
        }
    }
}

fn coalesce_from_base(entry: &mut Option<FileMeta>, base: &Option<FileMeta>) {
    if let (Some(meta), Some(base_meta)) = (entry.as_mut(), base.as_ref()) {
        if meta.mode.is_none() {
            meta.mode = base_meta.mode;
        }
    }
}

fn coalesce_between(a: &mut Option<FileMeta>, b: &mut Option<FileMeta>) {
    if let (Some(a_meta), Some(b_meta)) = (a.as_mut(), b.as_mut()) {
        if a_meta.hash == b_meta.hash {
            if a_meta.mode.is_none() {
                a_meta.mode = b_meta.mode;
            }
            if b_meta.mode.is_none() {
                b_meta.mode = a_meta.mode;
            }
        }
    }
}

fn shared_base(plan_a: &Plan, plan_b: &Plan) -> Result<Option<Arc<Plan>>, MergeError> {
    match (&plan_a.base, &plan_b.base) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) if Arc::ptr_eq(a, b) => Ok(Some(Arc::clone(a))),
        _ => Err(MergeError::IncompatibleBase),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub path: String,
    pub base: Option<FileMeta>,
    pub a: Option<FileMeta>,
    pub b: Option<FileMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    IncompatibleBase,
    Conflicts(Vec<MergeConflict>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(hash: &str, mode: u32) -> FileMeta {
        FileMeta::new(hash, Some(mode))
    }

    fn meta_unknown(hash: &str) -> FileMeta {
        FileMeta::new(hash, None)
    }

    #[test]
    fn materialize_applies_layers_with_permissions() {
        let mut base = Plan::new("base", None);
        let mut init = Layer::new("init");
        init.changes.push(FileChange::Added {
            path: "/readme".into(),
            meta: meta("h1", 0o644),
        });
        base.apply_layer(init);

        let base_state = base.get_file_system_state();
        assert_eq!(base_state["/readme"], meta("h1", 0o644));

        let base_arc = Arc::new(base);
        let mut feature = Plan::clone_plan(&base_arc, "feature");
        let mut layer = Layer::new("perm");
        layer.changes.push(FileChange::PermissionChanged {
            path: "/readme".into(),
            mode: Some(0o600),
        });
        feature.apply_layer(layer);

        let state = feature.get_file_system_state();
        assert_eq!(state["/readme"], meta("h1", 0o600));
    }

    #[test]
    fn merge_three_way_prefers_single_side_change() {
        let mut base = Plan::new("base", None);
        let mut init = Layer::new("init");
        init.changes.push(FileChange::Added {
            path: "/file".into(),
            meta: meta("v1", 0o644),
        });
        base.apply_layer(init);
        let base_arc = Arc::new(base);

        let mut a = Plan::clone_plan(&base_arc, "A");
        let mut a_layer = Layer::new("a1");
        a_layer.changes.push(FileChange::Modified {
            path: "/file".into(),
            meta: meta("v2", 0o644),
        });
        a.apply_layer(a_layer);

        let b = Plan::clone_plan(&base_arc, "B");

        let merged = Plan::merge_three_way("M", &a, &b).expect("merge ok");
        let state = merged.get_file_system_state();
        assert_eq!(state["/file"], meta("v2", 0o644));
    }

    #[test]
    fn merge_three_way_detects_conflict() {
        let mut base = Plan::new("base", None);
        let mut init = Layer::new("init");
        init.changes.push(FileChange::Added {
            path: "/file".into(),
            meta: meta("v1", 0o644),
        });
        base.apply_layer(init);
        let base_arc = Arc::new(base);

        let mut a = Plan::clone_plan(&base_arc, "A");
        let mut a_layer = Layer::new("a1");
        a_layer.changes.push(FileChange::Modified {
            path: "/file".into(),
            meta: meta("va", 0o644),
        });
        a.apply_layer(a_layer);

        let mut b = Plan::clone_plan(&base_arc, "B");
        let mut b_layer = Layer::new("b1");
        b_layer.changes.push(FileChange::Modified {
            path: "/file".into(),
            meta: meta("vb", 0o644),
        });
        b.apply_layer(b_layer);

        let err = Plan::merge_three_way("M", &a, &b).unwrap_err();
        match err {
            MergeError::Conflicts(conflicts) => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].path, "/file");
            }
            _ => panic!("expected conflict"),
        }
    }

    #[test]
    fn merge_three_way_handles_permission_only_changes() {
        let mut base = Plan::new("base", None);
        let mut init = Layer::new("init");
        init.changes.push(FileChange::Added {
            path: "/script".into(),
            meta: meta("h1", 0o644),
        });
        base.apply_layer(init);
        let base_arc = Arc::new(base);

        let mut a = Plan::clone_plan(&base_arc, "A");
        let mut a_layer = Layer::new("perm");
        a_layer.changes.push(FileChange::PermissionChanged {
            path: "/script".into(),
            mode: Some(0o755),
        });
        a.apply_layer(a_layer);

        let b = Plan::clone_plan(&base_arc, "B");

        let merged = Plan::merge_three_way("M", &a, &b).expect("merge ok");
        let state = merged.get_file_system_state();
        assert_eq!(state["/script"], meta("h1", 0o755));
    }

    #[test]
    fn merge_three_way_ignores_unknown_mode() {
        let mut base = Plan::new("base", None);
        let mut init = Layer::new("init");
        init.changes.push(FileChange::Added {
            path: "/bin".into(),
            meta: meta("h1", 0o755),
        });
        base.apply_layer(init);
        let base_arc = Arc::new(base);

        let mut a = Plan::clone_plan(&base_arc, "A");
        let mut a_layer = Layer::new("a1");
        a_layer.changes.push(FileChange::Modified {
            path: "/bin".into(),
            meta: meta_unknown("h2"),
        });
        a.apply_layer(a_layer);

        let b = Plan::clone_plan(&base_arc, "B");

        let merged = Plan::merge_three_way("M", &a, &b).expect("merge ok");
        let state = merged.get_file_system_state();
        assert_eq!(state["/bin"], meta("h2", 0o755));
    }

    #[test]
    fn merge_three_way_rejects_incompatible_bases() {
        let base_a = Arc::new(Plan::new("baseA", None));
        let base_b = Arc::new(Plan::new("baseB", None));

        let a = Plan::clone_plan(&base_a, "A");
        let b = Plan::clone_plan(&base_b, "B");

        let err = Plan::merge_three_way("M", &a, &b).unwrap_err();
        assert!(matches!(err, MergeError::IncompatibleBase));
    }
}
