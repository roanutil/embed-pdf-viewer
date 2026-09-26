pub mod functions;
pub use functions::{pure_functions, repository_functions};
pub fn uuid_at(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
