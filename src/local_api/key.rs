//! The ElevenLabs API key, as the local API sees it: whether one is set.
//! Every transcript comes from ElevenLabs, so a request that would send
//! audio is refused at its headers while there is no key, before any audio
//! is uploaded. The key itself never passes through here or into a response.

use super::HttpResponse;

/// What a request that needs a key is told while none is set.
const MISSING: &str = "no ElevenLabs API key is set; save one in Settings or with `hex key set`, \
     or set ELEVENLABS_API_KEY";

/// Whether this local API can send audio to ElevenLabs. Read per request,
/// so a key saved while Hex runs applies to the next one.
#[derive(Clone, Copy)]
pub struct KeyGate {
    set: fn() -> bool,
}

impl Default for KeyGate {
    fn default() -> Self {
        Self {
            set: crate::transcription::models::is_ready,
        }
    }
}

impl KeyGate {
    /// Under another answer, so route tests need neither a key nor
    /// ElevenLabs.
    #[cfg(test)]
    pub(crate) fn gated(set: fn() -> bool) -> Self {
        Self { set }
    }

    pub(super) fn is_set(self) -> bool {
        (self.set)()
    }

    /// `409 api-key-missing` while no key is set.
    pub(super) fn gate(self) -> Result<(), HttpResponse> {
        if self.is_set() {
            return Ok(());
        }
        Err(HttpResponse::refusal(409, "api-key-missing", MISSING))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_without_a_key_is_told_how_to_set_one() {
        assert!(KeyGate::gated(|| true).gate().is_ok());
        let refusal = KeyGate::gated(|| false).gate().unwrap_err();
        assert_eq!(refusal.status, 409);
        let body: serde_json::Value = serde_json::from_slice(&refusal.body).unwrap();
        assert_eq!(body["code"], "api-key-missing");
        assert!(
            body["message"].as_str().unwrap().contains("hex key set"),
            "{body}"
        );
    }

    #[test]
    fn unit_tests_have_no_key() {
        assert!(!KeyGate::default().is_set());
    }
}
