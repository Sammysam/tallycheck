// sqlx::migrate! embeds migrations/sqlite at compile time; rebuild when a file is added or
// changed there, otherwise a new migration would silently be missing from the binary.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations/sqlite");
}
