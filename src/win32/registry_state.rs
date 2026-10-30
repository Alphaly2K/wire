//! Process-local keys, handles and storage quotas; no memory or host registry.
use std::collections::BTreeMap;
pub(crate) type Key = (u32, String);
#[derive(Default)]
pub(crate) struct Node {
    pub name: String,
    pub values: BTreeMap<String, Value>,
}
pub(crate) struct Value {
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}

pub(crate) struct Registry {
    pub nodes: BTreeMap<Key, Node>,
    handles: BTreeMap<u64, (Option<Key>, u32)>,
    pub next: u64,
    end: u64,
    pub budget: Option<Budget>,
}
pub(crate) struct Budget {
    pub nodes: usize,
    pub values: usize,
    pub bytes: usize,
}
pub(crate) struct Open {
    key: Key,
    additions: Vec<(Key, String)>,
    access: u32,
    pub handle: u64,
    pub created: bool,
}
pub(crate) fn root(handle: u64) -> Option<u32> {
    let value = handle as u32;
    (matches!(value, 0x80000000..=0x80000003 | 0x80000005)
        && (handle == u64::from(value) || handle == 0xffffffff00000000 | u64::from(value)))
    .then(|| value - 0x80000000)
}
impl Registry {
    pub(crate) fn new(start: u64, end: u64, budget: Option<Budget>) -> Self {
        Self {
            nodes: [0, 1, 2, 3, 5]
                .into_iter()
                .map(|root| ((root, String::new()), Node::default()))
                .collect(),
            handles: BTreeMap::new(),
            next: start,
            end,
            budget,
        }
    }
    pub(crate) fn resolve(&self, handle: u64) -> Result<(Key, u32), u32> {
        if let Some(root) = root(handle) {
            Ok((
                (root, String::new()),
                if root == 1 { 0xf003f } else { 0x20019 },
            ))
        } else {
            let (key, access) = self.handles.get(&handle).ok_or(6u32)?;
            Ok((key.clone().ok_or(1018u32)?, *access))
        }
    }
    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if root(handle).is_some() || self.handles.remove(&handle).is_some() {
            Ok(())
        } else {
            Err(6)
        }
    }
    pub(crate) fn usage(&self) -> (usize, usize) {
        self.nodes
            .iter()
            .fold((0, 0), |(values, bytes), ((_, path), node)| {
                (
                    values + node.values.len(),
                    bytes
                        + path.len()
                        + node.name.len()
                        + node
                            .values
                            .iter()
                            .map(|(name, value)| name.len() + value.name.len() + value.data.len())
                            .sum::<usize>(),
                )
            })
    }
    pub(crate) fn key(&self, base: &Key, child: &str) -> Result<Key, u32> {
        if child.starts_with('\\')
            || child.ends_with('\\')
            || child.contains("\\\\")
            || child.len() > 4096
        {
            return Err(87);
        }
        let path = if child.is_empty() {
            base.1.clone()
        } else if base.1.is_empty() {
            child.to_lowercase()
        } else {
            format!("{}\\{}", base.1, child.to_lowercase())
        };
        if self.budget.is_some() && path.len() > 4096 {
            return Err(87);
        }
        Ok((base.0, path))
    }
    pub(crate) fn prepare_open(
        &self,
        base: &Key,
        parent_access: u32,
        child: &str,
        create: bool,
        access: u32,
    ) -> Result<Open, u32> {
        let key = self.key(base, child)?;
        let created = !self.nodes.contains_key(&key);
        if !create && created {
            return Err(2);
        }
        if create && created && (base.0 != 1 || parent_access & 4 == 0) {
            return Err(5);
        }
        if base.0 != 1 && access & (2 | 4 | 0x10000 | 0x40000 | 0x80000) != 0 {
            return Err(5);
        }
        if self.handles.len() >= 4096 || self.next >= self.end {
            return Err(8);
        }
        let mut additions = Vec::new();
        if create && created {
            let path = if base.1.is_empty() {
                child.to_owned()
            } else {
                format!("{}\\{child}", base.1)
            };
            let mut prefix = String::new();
            for component in path.split('\\') {
                if !prefix.is_empty() {
                    prefix.push('\\');
                }
                prefix.push_str(&component.to_lowercase());
                let prefix_key = (key.0, prefix.clone());
                if !self.nodes.contains_key(&prefix_key) {
                    additions.push((prefix_key, component.to_owned()));
                }
            }
            if let Some(budget) = &self.budget {
                let (mut nodes, mut bytes) = (self.nodes.len(), self.usage().1);
                for ((_, prefix), name) in &additions {
                    nodes += 1;
                    bytes += prefix.len() + name.len();
                    if nodes > budget.nodes || bytes > budget.bytes {
                        return Err(8);
                    }
                }
            }
        }
        Ok(Open {
            key,
            additions,
            access,
            handle: self.next,
            created,
        })
    }
    pub(crate) fn commit_open(&mut self, open: Open) {
        for (key, name) in open.additions {
            self.nodes.insert(
                key,
                Node {
                    name,
                    values: BTreeMap::new(),
                },
            );
        }
        self.handles
            .insert(open.handle, (Some(open.key), open.access));
        self.next += 16;
    }
    pub(crate) fn check_value(&self, key: &Key, name: &str, length: usize) -> Result<(), u32> {
        if let Some(budget) = &self.budget {
            let (values, bytes) = self.usage();
            let normalized = name.to_lowercase();
            let old = self.nodes[key].values.get(&normalized);
            let previous = old.map_or(0, |value| {
                normalized.len() + value.name.len() + value.data.len()
            });
            let stored_name = old.map_or(name, |value| value.name.as_str());
            if values + usize::from(old.is_none()) > budget.values
                || bytes - previous + normalized.len() + stored_name.len() + length > budget.bytes
            {
                return Err(8);
            }
        }
        Ok(())
    }
    pub(crate) fn set_value(&mut self, key: &Key, name: String, kind: u32, data: Vec<u8>) {
        let value = self
            .nodes
            .get_mut(key)
            .unwrap()
            .values
            .entry(name.to_lowercase())
            .or_insert_with(|| Value {
                name,
                kind,
                data: Vec::new(),
            });
        value.kind = kind;
        value.data = data;
    }
    pub(crate) fn children<'a>(&'a self, key: &'a Key) -> impl Iterator<Item = &'a Node> {
        self.nodes.iter().filter_map(move |((root, path), node)| {
            let parent = path.rsplit_once('\\').map_or("", |(parent, _)| parent);
            (*root == key.0 && !path.is_empty() && parent == key.1).then_some(node)
        })
    }
    pub(crate) fn delete_key(&mut self, key: &Key, tree: bool, keep_key: bool) -> Result<(), u32> {
        if !self.nodes.contains_key(key) {
            return Err(2);
        }
        if !keep_key && key.1.is_empty() || !tree && self.children(key).next().is_some() {
            return Err(5);
        }
        let removed = |other: &Key| {
            other.0 == key.0
                && ((!keep_key && other == key)
                    || (key.1.is_empty() && !other.1.is_empty())
                    || other
                        .1
                        .strip_prefix(&key.1)
                        .is_some_and(|tail| tail.starts_with('\\')))
        };
        // Deleted handles retain their identity until close, even after a path is reused.
        for (target, _) in self.handles.values_mut() {
            if target.as_ref().is_some_and(removed) {
                *target = None;
            }
        }
        self.nodes.retain(|other, _| !removed(other));
        if keep_key {
            self.nodes.get_mut(key).unwrap().values.clear();
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
        self.nodes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_nested_key_quota_failure_keeps_parents_and_handle_identity_unpublished() {
        for budget in [
            Budget {
                nodes: 6,
                values: 4,
                bytes: 1024,
            },
            Budget {
                nodes: 8,
                values: 4,
                bytes: 10,
            },
        ] {
            let mut registry = Registry::new(0x10000, 0x20000, Some(budget));
            let (root, access) = registry.resolve(0x80000001).unwrap();
            assert_eq!(
                registry
                    .prepare_open(&root, access, "alpha\\beta", true, 3)
                    .err(),
                Some(8)
            );
            assert_eq!(registry.nodes.len(), 5);
            assert_eq!(registry.usage(), (0, 0));
            assert!(registry.handles.is_empty());
            let pending = registry
                .prepare_open(&root, access, "alpha", true, 3)
                .unwrap();
            assert_eq!(pending.handle, 0x10000);
            registry.commit_open(pending);
            assert_eq!(registry.nodes.len(), 6);
        }
    }

    #[test]
    fn registry_replacement_and_deletion_release_value_and_byte_capacity() {
        let mut registry = Registry::new(
            0x10000,
            0x20000,
            Some(Budget {
                nodes: 8,
                values: 1,
                bytes: 17,
            }),
        );
        let (key, _) = registry.resolve(0x80000001).unwrap();
        registry.check_value(&key, "n", 15).unwrap();
        registry.set_value(&key, "n".into(), 3, vec![42; 15]);
        assert_eq!(registry.check_value(&key, "n", 16), Err(8));
        assert_eq!(registry.check_value(&key, "other", 0), Err(8));
        registry.check_value(&key, "n", 0).unwrap();
        registry.set_value(&key, "n".into(), 3, Vec::new());
        assert_eq!(registry.usage(), (1, 2));
        registry.nodes.get_mut(&key).unwrap().values.remove("n");
        registry.check_value(&key, "other", 7).unwrap();
        registry.clear();
        assert!(registry.nodes.is_empty());
        assert!(registry.handles.is_empty());
    }

    #[test]
    fn deleting_a_tree_invalidates_handles_without_retargeting_after_recreation() {
        let mut registry = Registry::new(0x10000, 0x20000, None);
        let (root, access) = registry.resolve(0x80000001).unwrap();
        let pending = registry
            .prepare_open(&root, access, "Parent\\Child", true, access)
            .unwrap();
        let handle = pending.handle;
        registry.commit_open(pending);
        let (key, _) = registry.resolve(handle).unwrap();
        registry.set_value(&key, "MixedCase".into(), 3, vec![1]);
        registry.set_value(&key, "MIXEDCASE".into(), 3, vec![2]);
        assert_eq!(registry.nodes[&key].values["mixedcase"].name, "MixedCase");
        assert_eq!(registry.children(&root).next().unwrap().name, "Parent");
        let parent = registry.key(&root, "parent").unwrap();
        assert_eq!(registry.delete_key(&parent, false, false), Err(5));
        registry.delete_key(&parent, true, true).unwrap();
        assert_eq!(registry.resolve(handle), Err(1018));
        assert_eq!(registry.children(&root).next().unwrap().name, "Parent");
        let pending = registry
            .prepare_open(&root, access, "parent\\CHILD", true, access)
            .unwrap();
        let replacement = pending.handle;
        registry.commit_open(pending);
        assert_ne!(handle, replacement);
        assert_eq!(registry.resolve(handle), Err(1018));
        assert!(registry.nodes[&key].values.is_empty());
        registry.close(handle).unwrap();
        assert_eq!(registry.resolve(handle), Err(6));
        assert_eq!(registry.close(handle), Err(6));
        registry.delete_key(&parent, true, false).unwrap();
        assert_eq!(registry.resolve(replacement), Err(1018));
        assert_eq!(registry.usage(), (0, 0));
    }
}
