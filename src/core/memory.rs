pub struct MemoryState {
    active_profile: String,
}

impl MemoryState {
    pub fn new(active_profile: String) -> Self {
        Self { active_profile }
    }

    pub fn summary(&self) -> String {
        format!("active profile = {}", self.active_profile)
    }
}

