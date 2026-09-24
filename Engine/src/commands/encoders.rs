use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};

#[tauri::command]
pub fn encode_decode(algorithm: String, mode: String, input: String) -> Result<String, String> {
    let trimmed = input.trim();

    match (algorithm.as_str(), mode.as_str()) {
        ("base64", "encode") => Ok(STANDARD.encode(input.as_bytes())),
        ("base64", "decode") => {
            let bytes = STANDARD
                .decode(trimmed)
                .map_err(|e| format!("Invalid Base64: {}", e))?;
            String::from_utf8(bytes).map_err(|e| format!("Not valid UTF-8: {}", e))
        }

        ("base64url", "encode") => Ok(URL_SAFE_NO_PAD.encode(input.as_bytes())),
        ("base64url", "decode") => {
            let bytes = URL_SAFE_NO_PAD
                .decode(trimmed)
                .map_err(|e| format!("Invalid Base64URL: {}", e))?;
            String::from_utf8(bytes).map_err(|e| format!("Not valid UTF-8: {}", e))
        }

        ("hex", "encode") => Ok(hex::encode(input.as_bytes())),
        ("hex", "decode") => {
            let bytes = hex::decode(trimmed).map_err(|e| format!("Invalid hex: {}", e))?;
            String::from_utf8(bytes).map_err(|e| format!("Not valid UTF-8: {}", e))
        }

        ("url", "encode") => Ok(urlencoding::encode(&input).into_owned()),
        ("url", "decode") => urlencoding::decode(&input)
            .map(|s| s.into_owned())
            .map_err(|e| format!("Invalid URL encoding: {}", e)),

        ("html", "encode") => Ok(html_escape::encode_text(&input).into_owned()),
        ("html", "decode") => Ok(html_escape::decode_html_entities(&input).into_owned()),

        ("binary", "encode") => Ok(input
            .as_bytes()
            .iter()
            .map(|b| format!("{:08b}", b))
            .collect::<Vec<_>>()
            .join(" ")),
        ("binary", "decode") => {
            let cleaned: String = trimmed.split_whitespace().collect();
            if !cleaned.len().is_multiple_of(8) {
                return Err("Binary input length must be a multiple of 8".into());
            }
            let bytes: Result<Vec<u8>, _> = (0..cleaned.len())
                .step_by(8)
                .map(|i| u8::from_str_radix(&cleaned[i..i + 8], 2))
                .collect();
            let bytes = bytes.map_err(|e| format!("Invalid binary: {}", e))?;
            String::from_utf8(bytes).map_err(|e| format!("Not valid UTF-8: {}", e))
        }

        ("rot13", _) => Ok(input
            .chars()
            .map(|c| match c {
                'a'..='m' | 'A'..='M' => ((c as u8) + 13) as char,
                'n'..='z' | 'N'..='Z' => ((c as u8) - 13) as char,
                _ => c,
            })
            .collect()),

        _ => Err(format!("Unknown algorithm: {}", algorithm)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(algorithm: &str, mode: &str, input: &str) -> Result<String, String> {
        encode_decode(algorithm.to_string(), mode.to_string(), input.to_string())
    }

    #[test]
    fn base64_encodes_known_value_and_round_trips() {
        assert_eq!(run("base64", "encode", "abc").unwrap(), "YWJj");
        let original = "Hello, KeepItLocal! 123";
        let encoded = run("base64", "encode", original).unwrap();
        assert_eq!(run("base64", "decode", &encoded).unwrap(), original);
    }

    #[test]
    fn base64url_avoids_unsafe_characters_and_round_trips() {
        let original = ">>>? playground";
        let encoded = run("base64url", "encode", original).unwrap();
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
        assert_eq!(run("base64url", "decode", &encoded).unwrap(), original);
    }

    #[test]
    fn hex_encodes_known_value_and_round_trips() {
        assert_eq!(run("hex", "encode", "abc").unwrap(), "616263");
        let original = "round trip me";
        let encoded = run("hex", "encode", original).unwrap();
        assert_eq!(run("hex", "decode", &encoded).unwrap(), original);
    }

    #[test]
    fn binary_encodes_known_value_and_round_trips() {
        assert_eq!(run("binary", "encode", "A").unwrap(), "01000001");
        let original = "bits";
        let encoded = run("binary", "encode", original).unwrap();
        assert_eq!(run("binary", "decode", &encoded).unwrap(), original);
    }

    #[test]
    fn url_encoding_round_trips() {
        let original = "a b & c=d/e";
        let encoded = run("url", "encode", original).unwrap();
        assert_eq!(run("url", "decode", &encoded).unwrap(), original);
    }

    #[test]
    fn rot13_is_its_own_inverse() {
        assert_eq!(run("rot13", "encode", "Hello").unwrap(), "Uryyb");
        let original = "The quick brown fox";
        let once = run("rot13", "encode", original).unwrap();
        assert_eq!(run("rot13", "encode", &once).unwrap(), original);
    }

    #[test]
    fn invalid_base64_input_is_rejected() {
        assert!(run("base64", "decode", "@@@not-base64@@@").is_err());
    }

    #[test]
    fn binary_decode_rejects_non_multiple_of_eight() {
        assert!(run("binary", "decode", "0101").is_err());
    }

    #[test]
    fn unknown_algorithm_is_rejected() {
        assert!(run("rot47", "encode", "anything").is_err());
    }
}
