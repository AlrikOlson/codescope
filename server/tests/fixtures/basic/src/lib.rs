use crate::types::Config;

/// Greet a user by name.
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

/// Process configuration and return status.
pub fn process(config: &Config) -> Result<String, String> {
    if config.name.is_empty() {
        Err("Name cannot be empty".to_string())
    } else {
        Ok(format!("Processed: {}", config.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greet() {
        assert_eq!(greet("World"), "Hello, World!");
    }
}
