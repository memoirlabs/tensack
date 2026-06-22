#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    name: String,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
