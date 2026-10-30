use epdf_engine::host::{HostResponse, Payloads, RequestEffect};

use crate::abi::{Failure, Status};
use crate::config::Options;

pub(crate) fn initialize_fonts(
    options: &Options,
    payloads: Payloads,
    mut call: impl FnMut(&str, Payloads) -> HostResponse,
) -> Result<(), Failure> {
    let fonts = &options.startup_fonts;
    if fonts.len() > 64 || fonts.len() != payloads.len() {
        return Err(Failure::new(
            Status::InvalidArgument,
            "one binary payload is required per startup font",
        ));
    }
    let mut used = [false; 64];
    for (index, font) in fonts.iter().enumerate() {
        if font.payload_index >= payloads.len()
            || used[font.payload_index]
            || fonts[..index].iter().any(|prior| prior.key == font.key)
        {
            return Err(Failure::new(
                Status::InvalidArgument,
                "startup font keys and payload indices must be unique",
            ));
        }
        used[font.payload_index] = true;
    }
    let mut available = Vec::new();
    available
        .try_reserve_exact(payloads.len())
        .map_err(|_| Failure::new(Status::LimitExceeded, "startup font allocation failed"))?;
    for payload in payloads {
        available.push(Some(payload));
    }
    let mut job_id = 1u64;
    for font in fonts {
        let data = available[font.payload_index].take().ok_or_else(|| {
            Failure::new(Status::InvalidArgument, "startup font payload is missing")
        })?;
        let request = serde_json::json!({
            "kind": "fonts.register",
            "effect": RequestEffect::RuntimeWrite.as_str(),
            "jobId": job_id,
            "fontKey": font.key,
            "familyName": font.family_name,
            "weight": font.weight,
            "italic": font.italic.map_or(-1, i32::from),
        });
        check(call(&request.to_string(), vec![data]))?;
        job_id += 1;
        if font.fallback {
            let request = serde_json::json!({
                "kind": "fonts.addFallback", "effect": RequestEffect::RuntimeWrite.as_str(),
                "jobId": job_id, "fontKey": font.key,
            });
            check(call(&request.to_string(), Vec::new()))?;
            job_id += 1;
        }
    }
    Ok(())
}

fn check(response: HostResponse) -> Result<(), Failure> {
    let parsed: serde_json::Value = serde_json::from_str(&response.json)
        .map_err(|_| Failure::new(Status::RuntimeUnavailable, "invalid startup font response"))?;
    if parsed.get("error").is_some() {
        return Err(Failure { status: Status::RuntimeUnavailable, response });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_binary_fonts_and_fallbacks_use_the_existing_wire_descriptions() {
        let options: Options = serde_json::from_value(serde_json::json!({
            "startupFonts": [
                {"key":"plain", "payloadIndex":1},
                {"key":"fallback", "payloadIndex":0, "fallback":true}
            ]
        }))
        .unwrap();
        let mut calls = Vec::new();
        initialize_fonts(&options, vec![vec![11], vec![22]], |request, payloads| {
            calls.push((serde_json::from_str::<serde_json::Value>(request).unwrap(), payloads));
            HostResponse { json: "{\"result\":null}".into(), payloads: vec![] }
        })
        .unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0["kind"], "fonts.register");
        assert_eq!(calls[0].0["fontKey"], "plain");
        assert_eq!(calls[0].0["italic"], -1);
        assert_eq!(calls[0].0["effect"], "runtimeWrite");
        assert_eq!(calls[0].1, [vec![22]]);
        assert_eq!(calls[1].0["fontKey"], "fallback");
        assert_eq!(calls[1].1, [vec![11]]);
        assert_eq!(calls[2].0["kind"], "fonts.addFallback");
        assert_eq!(calls[2].0["fontKey"], "fallback");
        assert!(calls[2].1.is_empty());
    }

    #[test]
    fn invalid_payload_mapping_is_refused_before_any_font_registration() {
        let options: Options = serde_json::from_value(serde_json::json!({
            "startupFonts": [{"key":"a", "payloadIndex":0}, {"key":"b", "payloadIndex":0}]
        }))
        .unwrap();
        let mut called = false;
        let error = initialize_fonts(&options, vec![vec![1], vec![2]], |_, _| {
            called = true;
            HostResponse { json: "{\"result\":null}".into(), payloads: vec![] }
        })
        .unwrap_err();
        assert_eq!(error.status, Status::InvalidArgument);
        assert!(!called);
    }
}
