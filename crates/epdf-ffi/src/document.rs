//! Confirmed event origin shared by document plugins.
use epdf_kernel::Error;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum DocumentEventOriginKind {
    Local,
    Remote,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentEventTransaction {
    pub id: String,
    pub index: u32,
    pub count: u32,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentEventOrigin {
    pub kind: DocumentEventOriginKind,
    pub session_id: String,
    pub subject: Option<String>,
    pub timestamp_ms: f64,
    pub server_id: Option<f64>,
    pub transaction: Option<DocumentEventTransaction>,
    pub undo_of: Option<String>,
}
impl DocumentEventOrigin {
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let invalid = || {
            Error::named("InvalidState", "confirmed event origin does not decode")
                .with("code", "invalid-state")
        };
        let text = |field: &str| {
            value.get(field).and_then(Value::as_str).map(str::to_owned).ok_or_else(invalid)
        };
        let optional_text = |field: &str| -> Result<Option<String>, Error> {
            match value.get(field) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.clone())),
                _ => Err(invalid()),
            }
        };
        let transaction = value
            .get("tx")
            .filter(|tx| !tx.is_null())
            .map(|tx| {
                let whole = |field| {
                    tx.get(field)
                        .and_then(Value::as_f64)
                        .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX))
                        .map(|n| n as u32)
                        .ok_or_else(invalid)
                };
                Ok::<_, Error>(DocumentEventTransaction {
                    id: tx.get("id").and_then(Value::as_str).ok_or_else(invalid)?.into(),
                    index: whole("index")?,
                    count: whole("count")?,
                })
            })
            .transpose()?;
        let server_id = match value.get("serverId") {
            None | Some(Value::Null) => None,
            Some(number) => Some(number.as_f64().ok_or_else(invalid)?),
        };
        Ok(Self {
            kind: match text("kind")?.as_str() {
                "local" => DocumentEventOriginKind::Local,
                "remote" => DocumentEventOriginKind::Remote,
                _ => return Err(invalid()),
            },
            session_id: text("sessionId")?,
            subject: optional_text("sub")?,
            timestamp_ms: value.get("ts").and_then(Value::as_f64).ok_or_else(invalid)?,
            server_id,
            transaction,
            undo_of: optional_text("undoOf")?,
        })
    }
}
