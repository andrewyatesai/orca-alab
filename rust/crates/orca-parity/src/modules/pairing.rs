//! Parity dispatch for `orca_relay::pairing` vs `src/shared/pairing.ts`.

use orca_relay::{
    decode_pairing_offer, encode_pairing_offer, parse_pairing_code, PairingOffer,
    PAIRING_OFFER_VERSION,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "encodePairingOffer" => Value::String(encode_pairing_offer(&offer_from_input(input))),
        // TS `decodePairingOffer` throws on a bad URL/payload and the TS adapter
        // maps that throw to null, so `Err` must produce the same null image.
        "decodePairingOffer" => match decode_pairing_offer(input.as_str().unwrap_or_default()) {
            Ok(offer) => offer_to_json(&offer),
            Err(_) => Value::Null,
        },
        "parsePairingCode" => match parse_pairing_code(input.as_str().unwrap_or_default()) {
            Some(offer) => offer_to_json(&offer),
            None => Value::Null,
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `PairingOffer` object (camelCase keys).
fn offer_to_json(offer: &PairingOffer) -> Value {
    json!({
        "v": offer.v,
        "endpoint": offer.endpoint,
        "deviceToken": offer.device_token,
        "publicKeyB64": offer.public_key_b64,
    })
}

fn offer_from_input(input: &Value) -> PairingOffer {
    PairingOffer {
        v: input
            .get("v")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| u64::from(PAIRING_OFFER_VERSION)) as u32,
        endpoint: string_field(input, "endpoint"),
        device_token: string_field(input, "deviceToken"),
        public_key_b64: string_field(input, "publicKeyB64"),
    }
}

fn string_field(input: &Value, key: &str) -> String {
    input.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}
