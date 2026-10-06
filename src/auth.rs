use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

// Public native-client compatibility identifiers, also used by python-tidal.
pub const PKCE_CLIENT_ID: &str = "6BDSRdpK9hqEBTgU";
pub const PKCE_CLIENT_SECRET: &str = "xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=";
pub const REDIRECT: &str = "https://tidal.com/android/login/auth";

pub struct Pkce {
    pub verifier: String,
    pub unique_key: String,
    state: String,
    created: Instant,
}

impl Pkce {
    pub fn new() -> Self {
        Self {
            verifier: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()),
            unique_key: format!("{:016x}", rand::random::<u64>()),
            state: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()),
            created: Instant::now(),
        }
    }

    pub fn url(&self) -> String {
        let mut url = reqwest::Url::parse("https://login.tidal.com/authorize").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("client_id", PKCE_CLIENT_ID),
            ("lang", "EN"),
            ("appMode", "android"),
            ("client_unique_key", &self.unique_key),
            ("state", &self.state),
            (
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes())),
            ),
            ("code_challenge_method", "S256"),
            ("restrict_signup", "true"),
        ]);
        url.into()
    }

    pub fn code(&self, redirect: &str) -> Result<String> {
        ensure!(
            self.created.elapsed() < Duration::from_secs(600),
            "Authorization expired. Start lossless sign-in again."
        );
        let url = reqwest::Url::parse(redirect.trim())
            .context("Paste the complete redirected TIDAL URL")?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("tidal.com")
                && url.port().is_none()
                && url.path() == "/android/login/auth"
                && url.username().is_empty()
                && url.password().is_none(),
            "This is not the TIDAL authorization redirect URL"
        );
        let pairs: Vec<_> = url.query_pairs().collect();
        let states: Vec<_> = pairs.iter().filter(|(k, _)| k == "state").collect();
        ensure!(
            states.len() == 1 && states[0].1 == self.state,
            "Authorization state does not match. Use the URL opened by this sign-in attempt."
        );
        let codes: Vec<_> = pairs.iter().filter(|(k, _)| k == "code").collect();
        ensure!(
            codes.len() == 1 && !codes[0].1.is_empty(),
            "TIDAL returned no authorization code"
        );
        Ok(codes[0].1.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_pkce_redirect_and_state() {
        let p = Pkce::new();
        assert_eq!(p.verifier.len(), 43);
        let good = format!("{REDIRECT}?code=test&state={}", p.state);
        assert_eq!(p.code(&good).unwrap(), "test");
        assert!(
            p.code(&good.replace("tidal.com", "tidal.com.evil.test"))
                .is_err()
        );
        assert!(p.code(&good.replace("https:", "http:")).is_err());
        assert!(
            p.code(&format!("{REDIRECT}?code=test&state=wrong"))
                .is_err()
        );
        assert!(p.code(&format!("{good}&code=duplicate")).is_err());
        assert!(p.code(&format!("{REDIRECT}?code=test")).is_err());
        assert!(p.url().contains("code_challenge_method=S256"));
    }
}
