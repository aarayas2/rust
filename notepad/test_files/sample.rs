// Sample Rust file for testing incremental syntax highlighting
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TokenStats {
    pub total_lines: usize,
    pub elapsed_ms: f64,
}

impl TokenStats {
    /* Multi-line block comment:
       verifying context stack push/pop across lines */
    pub fn compute(lines: &[&str]) -> Self {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &line in lines {
            if !line.is_empty() {
                *counts.entry(line).or_insert(0) += 1;
            }
        }
        println!("Processed {} unique lines", counts.len());
        Self {
            total_lines: lines.len(),
            elapsed_ms: 0.42,
        }
    }
}
