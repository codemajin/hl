//! A sample Rust file that the integration tests read, so that their input
//! does not change with the project's own files.

use std::collections::HashMap;

/// The greeting for `name`.
#[must_use]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

fn main() {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for word in "a b a".split(' ') {
        *counts.entry(word).or_insert(0) += 1;
    }
    // Prints the greeting and the count of "a".
    println!("{} {}", greet("world"), counts["a"]);
}
