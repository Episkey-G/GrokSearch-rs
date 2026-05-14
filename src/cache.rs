use std::collections::{HashMap, VecDeque};

use crate::model::source::Source;

#[derive(Debug, Clone)]
pub struct SourceCache {
    max_size: usize,
    order: VecDeque<String>,
    values: HashMap<String, Vec<Source>>,
}

impl SourceCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size: max_size.max(1),
            order: VecDeque::new(),
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, session_id: String, sources: Vec<Source>) {
        if self.values.contains_key(&session_id) {
            self.order.retain(|existing| existing != &session_id);
        }
        self.order.push_back(session_id.clone());
        self.values.insert(session_id, sources);

        while self.order.len() > self.max_size {
            if let Some(oldest) = self.order.pop_front() {
                self.values.remove(&oldest);
            }
        }
    }

    pub fn get(&mut self, session_id: &str) -> Option<Vec<Source>> {
        let sources = self.values.get(session_id).cloned()?;
        self.order.retain(|existing| existing != session_id);
        self.order.push_back(session_id.to_string());
        Some(sources)
    }
}
