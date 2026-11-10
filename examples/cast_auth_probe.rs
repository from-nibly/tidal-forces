//! Isolated P7a authentication / explicitly opted-in synthetic-tone probe.
//! Never part of the player executable; tone testing needs separate permission.
//! Run without the explicit flag to fail closed without networking.

#[allow(dead_code, unused_imports)]
#[path = "cast_auth/proof.rs"]
mod auth;
#[allow(dead_code)]
#[path = "../src/cast_control.rs"]
mod cast_control;
#[path = "../src/cast_media.rs"]
mod cast_media;

fn main() -> std::process::ExitCode {
    match auth::inspect() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("{reason}");
            std::process::ExitCode::FAILURE
        }
    }
}
