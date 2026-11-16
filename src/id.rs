//! The UUIDs saaio names flows and harness sessions by, each its own type

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A flow's id: the name of its log, `SAAIO_FLOW_ID`, the key the UI and server know it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FlowId(Uuid);

impl FlowId {
    pub fn generate() -> FlowId {
        FlowId(Uuid::new_v4())
    }

    pub fn parse(text: &str) -> Result<FlowId, NotAnId> {
        canonical(text).map(FlowId)
    }
}

impl fmt::Display for FlowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for FlowId {
    type Err = NotAnId;

    fn from_str(text: &str) -> Result<FlowId, NotAnId> {
        FlowId::parse(text)
    }
}

impl TryFrom<String> for FlowId {
    type Error = NotAnId;

    fn try_from(text: String) -> Result<FlowId, NotAnId> {
        FlowId::parse(&text)
    }
}

impl From<FlowId> for String {
    fn from(id: FlowId) -> String {
        id.to_string()
    }
}

/// The id a harness session resumes by: the master's, a Codex worker's, a Claude worker's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn generate() -> SessionId {
        SessionId(Uuid::new_v4())
    }

    pub fn parse(text: &str) -> Result<SessionId, NotAnId> {
        canonical(text).map(SessionId)
    }
}

/// A Claude Code worker's session is named after its flow.
impl From<FlowId> for SessionId {
    fn from(id: FlowId) -> SessionId {
        SessionId(id.0)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl TryFrom<String> for SessionId {
    type Error = NotAnId;

    fn try_from(text: String) -> Result<SessionId, NotAnId> {
        SessionId::parse(&text)
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> String {
        id.to_string()
    }
}

/// Text that is not an id in its one written form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is no id")]
pub struct NotAnId(String);

/// The UUID `text` writes, only in the form saaio writes it: lowercase and hyphenated, so the
/// text and the id round-trip and one id has one file name.
fn canonical(text: &str) -> Result<Uuid, NotAnId> {
    Uuid::try_parse(text)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == text)
        .ok_or_else(|| NotAnId(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "27bfad67-de17-45c0-88b9-8f75f3946781";

    #[test]
    fn an_id_and_its_text_round_trip() {
        let id = FlowId::generate();
        assert_eq!(FlowId::parse(&id.to_string()), Ok(id));
        assert_eq!(FlowId::parse(TEXT).unwrap().to_string(), TEXT);
        assert_eq!(SessionId::parse(TEXT).unwrap().to_string(), TEXT);
        assert_eq!(TEXT.parse::<FlowId>(), FlowId::parse(TEXT));
    }

    #[test]
    fn refuses_every_other_form() {
        for text in [
            "27BFAD67-DE17-45C0-88B9-8F75F3946781",
            "{27bfad67-de17-45c0-88b9-8f75f3946781}",
            "27bfad67de1745c088b98f75f3946781",
            "urn:uuid:27bfad67-de17-45c0-88b9-8f75f3946781",
            "27bfad67-de17-45c0-88b9",
            "fix-login",
            "",
        ] {
            assert_eq!(
                FlowId::parse(text),
                Err(NotAnId(text.to_string())),
                "{text}"
            );
            assert!(SessionId::parse(text).is_err(), "{text}");
        }
        assert_eq!(
            NotAnId("fix-login".to_string()).to_string(),
            "\"fix-login\" is no id"
        );
    }

    #[test]
    fn serializes_as_its_text() {
        let flow = FlowId::parse(TEXT).unwrap();
        let session = SessionId::parse(TEXT).unwrap();
        let text = TEXT.to_string();
        assert_eq!(
            serde_json::to_string(&flow).unwrap(),
            serde_json::to_string(&text).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&session).unwrap(),
            serde_json::to_string(&text).unwrap()
        );
        assert_eq!(
            postcard::to_stdvec(&flow).unwrap(),
            postcard::to_stdvec(&text).unwrap()
        );
        assert_eq!(
            postcard::to_stdvec(&session).unwrap(),
            postcard::to_stdvec(&text).unwrap()
        );
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(serde_json::from_str::<FlowId>(&json).unwrap(), flow);
        assert_eq!(serde_json::from_str::<SessionId>(&json).unwrap(), session);
        let bytes = postcard::to_stdvec(&text).unwrap();
        assert_eq!(postcard::from_bytes::<FlowId>(&bytes).unwrap(), flow);
        assert!(serde_json::from_str::<FlowId>("\"fix-login\"").is_err());
    }

    #[test]
    fn orders_as_its_text_does() {
        let mut ids: Vec<FlowId> = (0..64).map(|_| FlowId::generate()).collect();
        let mut texts: Vec<String> = ids.iter().map(ToString::to_string).collect();
        ids.sort();
        texts.sort();
        assert_eq!(
            ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            texts
        );
    }

    #[test]
    fn a_flow_names_its_session_by_its_own_text() {
        let flow = FlowId::parse(TEXT).unwrap();
        assert_eq!(SessionId::from(flow).to_string(), TEXT);
    }
}
