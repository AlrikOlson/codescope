/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub verbose: bool,
    pub max_retries: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "World".to_string(),
            verbose: false,
            max_retries: 3,
        }
    }
}

/// Status of an operation.
pub enum Status {
    Ok,
    Error(String),
    Pending,
}

impl Status {
    pub fn is_ok(&self) -> bool {
        matches!(self, Status::Ok)
    }
}

pub type Result<T> = std::result::Result<T, String>;
