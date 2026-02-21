use crate::lib::greet;
use crate::types::Config;

fn main() {
    let config = Config::default();
    println!("{}", greet(&config.name));
}

fn helper() -> i32 {
    42
}
