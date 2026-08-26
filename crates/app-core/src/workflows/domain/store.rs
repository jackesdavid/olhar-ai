//! A named collection of graphs, and how it is read and written.

use super::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Named-graph collection + persistence
// ---------------------------------------------------------------------------

/// The collection of named graphs (managed state on the Tauri side).
#[derive(Default)]
pub struct Workflows {
    pub graphs: Vec<Graph>,
}

impl Workflows {
    pub fn from_graphs(graphs: Vec<Graph>) -> Self {
        Workflows { graphs }
    }

    pub fn create_graph(&mut self, name: &str) -> Uuid {
        let g = Graph::new(name);
        let id = g.id;
        self.graphs.push(g);
        id
    }

    pub fn remove_graph(&mut self, id: Uuid) {
        self.graphs.retain(|g| g.id != id);
    }

    pub fn graph(&self, id: Uuid) -> Option<&Graph> {
        self.graphs.iter().find(|g| g.id == id)
    }

    pub fn graph_mut(&mut self, id: Uuid) -> Option<&mut Graph> {
        self.graphs.iter_mut().find(|g| g.id == id)
    }
}
