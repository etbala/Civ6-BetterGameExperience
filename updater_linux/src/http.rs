//! Téléchargements HTTPS par ureq (TLS rustls, certificats racine embarqués), comme `updater/src/http.rs`
//! par WinHTTP. Le proxy vient de `HTTPS_PROXY` / `ALL_PROXY`, lus par ureq.

use std::time::Duration;

/// Nom générique : l'API de GitHub refuse les requêtes sans User-Agent.
const USER_AGENT: &str = "BetterGameExperience-installer";
/// Comme `curl --retry 2` : trois essais en tout, pour une erreur réseau ou une erreur passagère du
/// serveur (408, 429, 5xx), jamais pour un 404.
const ATTEMPTS: u32 = 3;
/// Taille maximale d'une réponse : bien plus que les DLL, mais bornée.
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Corps de la réponse à `GET url`.
pub fn get(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
        match agent.get(url).call() {
            Ok(mut response) => match response.body_mut().with_config().limit(MAX_BYTES).read_to_vec() {
                Ok(data) => return Ok(data),
                Err(e) => last = e.to_string(),
            },
            Err(ureq::Error::StatusCode(code)) if transient(code) => last = format!("HTTP {code}"),
            Err(ureq::Error::StatusCode(code)) => return Err(format!("HTTP {code}")),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn transient(code: u16) -> bool {
    code == 408 || code == 429 || code >= 500
}

#[cfg(test)]
mod tests {
    use super::transient;

    #[test]
    fn erreurs_passageres() {
        assert!(transient(408) && transient(429) && transient(500) && transient(503));
        assert!(!transient(404) && !transient(403) && !transient(200));
    }
}
