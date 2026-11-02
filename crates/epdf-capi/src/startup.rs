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
        let status = match parsed["error"]["code"].as_str() {
            Some("InvalidArg" | "WireFormat") => Status::InvalidArgument,
            Some("PayloadTooLarge") => Status::LimitExceeded,
            _ => Status::RuntimeUnavailable,
        };
        return Err(Failure { status, response });
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

    #[test]
    fn a_registration_failing_partway_unwinds_earlier_fonts_and_joins_the_owner_thread() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::mpsc::{Sender, channel};
        use std::thread::{ThreadId, current};

        use crate::config::Limits;
        use crate::engine::Engine;

        struct Exit(Sender<ThreadId>);
        impl Drop for Exit {
            fn drop(&mut self) {
                let _ = self.0.send(current().id());
            }
        }
        thread_local! {
            static EXIT: RefCell<Option<Exit>> = const { RefCell::new(None) };
        }
        struct Fonts {
            keys: Vec<String>,
            unwound: Sender<(ThreadId, Vec<String>)>,
            _confined: Rc<()>,
        }
        impl Drop for Fonts {
            fn drop(&mut self) {
                let _ = self.unwound.send((current().id(), std::mem::take(&mut self.keys)));
            }
        }

        let options: Options = serde_json::from_value(serde_json::json!({
            "startupFonts": [
                {"key":"a", "payloadIndex":0, "fallback":true},
                {"key":"b", "payloadIndex":1},
                {"key":"c", "payloadIndex":2}
            ]
        }))
        .unwrap();
        let refusal = "{\"error\":{\"code\":\"InvalidArg\",\"message\":\"unloadable: b\"}}";
        let (exit_tx, exit_rx) = channel();
        let (unwound_tx, unwound_rx) = channel();
        let (calls_tx, calls_rx) = channel();
        let started = Engine::start(Limits::default(), move || {
            EXIT.with(|exit| *exit.borrow_mut() = Some(Exit(exit_tx)));
            let mut fonts = Fonts { keys: Vec::new(), unwound: unwound_tx, _confined: Rc::new(()) };
            let payloads = vec![vec![1], vec![2], vec![3]];
            initialize_fonts(&options, payloads, |request, _| {
                let request: serde_json::Value = serde_json::from_str(request).unwrap();
                let kind = request["kind"].as_str().unwrap().to_owned();
                let key = request["fontKey"].as_str().unwrap().to_owned();
                calls_tx.send((current().id(), kind.clone(), key.clone())).unwrap();
                if key == "b" {
                    return HostResponse { json: refusal.into(), payloads: vec![] };
                }
                if kind == "fonts.register" {
                    fonts.keys.push(key);
                }
                HostResponse { json: "{\"result\":null}".into(), payloads: vec![] }
            })?;
            Ok(fonts)
        });
        let Err(failure) = started else { panic!("a failed startup font must not start") };
        assert_eq!(failure.status, Status::InvalidArgument);
        assert_eq!(failure.response.json, refusal);

        let calls: Vec<_> = calls_rx.try_iter().collect();
        let owner = calls[0].0;
        assert_ne!(owner, current().id());
        let seen: Vec<_> =
            calls.iter().map(|(_, kind, key)| (kind.as_str(), key.as_str())).collect();
        assert_eq!(
            seen,
            [("fonts.register", "a"), ("fonts.addFallback", "a"), ("fonts.register", "b")]
        );
        assert!(calls.iter().all(|(thread, _, _)| *thread == owner));
        // Both signals were sent before `start` returned, so neither waits.
        assert_eq!(unwound_rx.try_recv().unwrap(), (owner, vec!["a".to_owned()]));
        assert_eq!(exit_rx.try_recv().unwrap(), owner);
    }
}
