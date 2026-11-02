use crate::abi::Status;
use crate::config::Limits;
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::host::{Host, HostResponse, JobEnvelope, Payloads, RequestEffect};
use serde_json::{Value, json};

pub(crate) fn validate(operation: u32, args: &Value, payload_count: usize) -> Result<(), Status> {
    let fields = args.as_object().ok_or(Status::InvalidArgument)?;
    let allowed: &[&str] = match operation {
        1 => &["password"],
        2 | 3 => &[],
        4 | 5 => &["page"],
        6 => &["page", "options"],
        7 => &["request"],
        _ => return Err(Status::InvalidArgument),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str()))
        || (operation != 1 && payload_count != 0)
    {
        return Err(Status::InvalidArgument);
    }
    Ok(())
}

fn invalid(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

pub(crate) fn request(
    operation: u32,
    mut args: Value,
    document: &str,
    job_id: u64,
    limits: &Limits,
) -> Result<Value, EngineError> {
    let kind = match operation {
        1 => "open.fatMem",
        2 => "close",
        3 => "pages.list",
        4 => "pages.text",
        5 => "pages.geometry",
        6 => "pages.render",
        7 => "search.query",
        _ => return Err(invalid("unknown operation")),
    };
    if operation == 1 {
        if !args["password"].is_null() && !args["password"].is_string() {
            return Err(invalid("password must be a string or null"));
        }
        args["identity"] = json!({});
        args["scope"] = json!(["*"]);
    }
    if operation == 6 {
        let options = args.as_object_mut().unwrap().entry("options").or_insert_with(|| json!({}));
        let options =
            options.as_object_mut().ok_or_else(|| invalid("render options must be an object"))?;
        let allowed = [
            "target",
            "viewport",
            "rotation",
            "background",
            "includeAnnotations",
            "includeFormFields",
            "maxOutputPixels",
            "usage",
        ];
        if options.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(invalid("unknown render option"));
        }
        if options.get("usage").is_some_and(|usage| usage != "display") {
            return Err(invalid("only display rendering is supported"));
        }
        if options.get("viewport").is_some_and(|viewport| viewport["kind"] != "scale") {
            return Err(invalid("render viewport must use scale"));
        }
        let requested = match options.get("maxOutputPixels") {
            None => limits.max_output_pixels,
            Some(value) => value
                .as_u64()
                .filter(|limit| *limit > 0)
                .ok_or_else(|| invalid("maxOutputPixels must be positive"))?,
        };
        let ceiling = limits
            .max_output_pixels
            .min(limits.max_result_bytes / 4)
            .min(limits.max_retained_result_bytes / 4);
        options.insert("maxOutputPixels".into(), json!(requested.min(ceiling)));
    }
    if operation == 7 {
        let search =
            args["request"].as_object_mut().ok_or_else(|| invalid("search requires a request"))?;
        let allowed = [
            "text",
            "matchCase",
            "matchDiacritics",
            "wholeWord",
            "ignoreWhitespace",
            "snippets",
            "from",
            "cursor",
            "limit",
            "regex",
        ];
        if search.keys().any(|key| !allowed.contains(&key.as_str()))
            || search.get("regex").is_some_and(|regex| regex != false)
        {
            return Err(invalid("only bounded literal search is supported"));
        }
        search.insert("regex".into(), json!(false));
        if let Some(cursor) = search.get("cursor").filter(|value| !value.is_null()) {
            let cursor: Value = serde_json::from_str(
                cursor.as_str().ok_or_else(|| invalid("invalid search cursor"))?,
            )
            .map_err(|_| invalid("invalid search cursor"))?;
            if cursor["document"] != document {
                return Err(invalid("search cursor belongs to a different document"));
            }
            let inner = cursor["cursor"]
                .as_str()
                .ok_or_else(|| invalid("invalid search cursor"))?
                .to_owned();
            search.insert("cursor".into(), json!(inner));
        }
    }
    args["kind"] = json!(kind);
    args["effect"] = json!(RequestEffect::of(kind).unwrap().as_str());
    args["jobId"] = json!(job_id);
    args["docId"] = json!(document);
    JobEnvelope::parse(kind, &args)?;
    Ok(args)
}

pub(crate) fn call(
    host: &mut Host,
    operation: u32,
    request: Value,
    payloads: Payloads,
    cancel: &CancelToken,
) -> Result<HostResponse, EngineError> {
    let document = request["docId"].as_str().unwrap();
    let mut response = host.call_with_cancel(&request.to_string(), payloads, cancel);
    let mut parsed: Value =
        serde_json::from_str(&response.json).map_err(|_| invalid("invalid Host response"))?;
    if operation == 1 {
        let locked = parsed["result"]["security"]["encryptionRequiresPassword"] == true;
        if parsed.get("error").is_some() || locked || cancel.is_cancelled() {
            close(host, document);
            if cancel.is_cancelled() {
                cancel.check()?;
            }
            if locked {
                let code = if parsed["result"]["passwordRejected"] == true {
                    EngineErrorCode::DocPasswordIncorrect
                } else {
                    EngineErrorCode::DocPasswordRequired
                };
                return Err(EngineError::with_message(
                    code,
                    "document password is required or incorrect",
                ));
            }
        }
    }
    if operation == 7
        && let Some(cursor) = parsed["result"]["nextCursor"].as_str()
    {
        parsed["result"]["nextCursor"] =
            json!(json!({"document":document,"cursor":cursor}).to_string());
        response.json = parsed.to_string();
    }
    Ok(response)
}

pub(crate) fn close(host: &mut Host, document: &str) {
    let request = json!({"kind":"close","docId":document,"jobId":0,"effect":"close"});
    host.call(&request.to_string(), vec![]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_refusals_remove_the_parked_host_session() {
        let bytes =
            include_bytes!("../../../tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf");
        let mut host = Host::new();
        for password in [None, Some("wrong")] {
            let request =
                request(1, json!({"password":password}), "locked", 1, &Limits::default()).unwrap();
            let failure = call(&mut host, 1, request, vec![bytes.to_vec()], &CancelToken::never())
                .unwrap_err();
            assert!(matches!(
                failure.code,
                EngineErrorCode::DocPasswordRequired | EngineErrorCode::DocPasswordIncorrect
            ));
            assert!(host.sessions().is_empty(), "password refusal retained a parked document");
        }
    }
}
