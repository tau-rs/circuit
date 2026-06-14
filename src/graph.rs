use std::collections::{BTreeSet, HashMap};

use crate::layer::{layer_of, Layer};

pub type ModuleId = usize;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub layer: Layer,
}

#[derive(Default)]
pub struct ArchGraph {
    modules: Vec<Module>,
    index: HashMap<String, ModuleId>,
    edges: BTreeSet<(ModuleId, ModuleId)>,
}

impl ArchGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Idempotent: returns the existing id or inserts a new module (layer assigned by convention).
    pub fn ensure_module(&mut self, name: &str) -> ModuleId {
        if let Some(&id) = self.index.get(name) {
            return id;
        }
        let id = self.modules.len();
        self.modules.push(Module { name: name.to_string(), layer: layer_of(name) });
        self.index.insert(name.to_string(), id);
        id
    }

    /// Adds a directed dependency edge. Self-edges are ignored; duplicates are deduped.
    pub fn add_edge(&mut self, from: ModuleId, to: ModuleId) {
        if from != to {
            self.edges.insert((from, to));
        }
    }

    pub fn module_id(&self, name: &str) -> Option<ModuleId> {
        self.index.get(name).copied()
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// Edges as a sorted, deduped vector.
    pub fn edges(&self) -> Vec<(ModuleId, ModuleId)> {
        self.edges.iter().copied().collect()
    }

    pub fn name(&self, id: ModuleId) -> &str {
        &self.modules[id].name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_module_is_idempotent_and_assigns_layer() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("domain");
        let b = g.ensure_module("domain");
        assert_eq!(a, b);
        assert_eq!(g.modules().len(), 1);
        assert_eq!(g.modules()[a].layer, Layer::Domain);
    }

    #[test]
    fn edges_are_deduped_and_self_edges_ignored() {
        let mut g = ArchGraph::new();
        let a = g.ensure_module("adapters");
        let d = g.ensure_module("domain");
        g.add_edge(a, d);
        g.add_edge(a, d);
        g.add_edge(d, d);
        assert_eq!(g.edges(), vec![(a, d)]);
    }
}
