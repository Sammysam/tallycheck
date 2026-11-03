//! D49 layer 3: "prove you own this PC". The desktop shell (`src-tauri`) asks Windows to confirm
//! that the person at the main PC is the Windows user running TallyCheck — Windows Hello (PIN,
//! fingerprint, face) or, without Hello, that user's Windows password — and the server only
//! passes the request through, like the updater (D45). That Windows account can already open
//! the books' files directly, so it is trusted to reset a TallyCheck password. A server started
//! without the shell has no verifier and does not offer this.

use std::future::Future;
use std::pin::Pin;

/// How the check ended. Only `Verified` allows a reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Verified,
    /// The person closed the window or gave the wrong PIN / password.
    NotVerified,
    /// Windows could not ask (a sentence for the user).
    Unavailable(String),
}

pub type VerifyFuture = Pin<Box<dyn Future<Output = Verification> + Send>>;

pub trait WindowsVerifier: Send + Sync {
    /// Shows Windows' own verification window over TallyCheck with `message`.
    fn verify(&self, message: String) -> VerifyFuture;
}
