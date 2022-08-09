use std::process::id;

#[derive(Default)]
pub struct Runtime {
    pid: u32,
}

impl Runtime {
    pub fn update_pid(&mut self) {
        self.pid = id();
    }
}
