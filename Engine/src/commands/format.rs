use serde_json::Value as JsonValue;

#[tauri::command]
pub fn convert_format(from: String, to: String, input: String) -> Result<String, String> {
    if input.trim().is_empty() {
        return Ok(String::new());
    }

    // Parse from source format into a generic JSON Value (universal intermediate)
    let value: JsonValue = match from.as_str() {
        "json" => serde_json::from_str(&input).map_err(|e| format!("Invalid JSON: {}", e))?,
        "yaml" => serde_yaml::from_str(&input).map_err(|e| format!("Invalid YAML: {}", e))?,
        "toml" => {
            let toml_value: toml::Value =
                toml::from_str(&input).map_err(|e| format!("Invalid TOML: {}", e))?;
            serde_json::to_value(toml_value)
                .map_err(|e| format!("TOML conversion failed: {}", e))?
        }
        "xml" => {
            let json_str = xml_to_json(&input)?;
            serde_json::from_str(&json_str).map_err(|e| format!("XML parse failed: {}", e))?
        }
        _ => return Err(format!("Unsupported source format: {}", from)),
    };

    // Serialize to target format
    match to.as_str() {
        "json" => serde_json::to_string_pretty(&value)
            .map_err(|e| format!("JSON serialize failed: {}", e)),
        "yaml" => {
            serde_yaml::to_string(&value).map_err(|e| format!("YAML serialize failed: {}", e))
        }
        "toml" => {
            // TOML requires a table at root, not array/scalar
            if !value.is_object() {
                return Err("TOML output requires an object/map at the root".into());
            }
            toml::to_string_pretty(&value).map_err(|e| format!("TOML serialize failed: {}", e))
        }
        "xml" => json_to_xml(&value),
        _ => Err(format!("Unsupported target format: {}", to)),
    }
}

// XML → JSON (best-effort: attributes become @attr, text becomes #text)
fn xml_to_json(xml: &str) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<(String, serde_json::Map<String, JsonValue>)> = Vec::new();
    let mut root: Option<(String, JsonValue)> = None;
    let mut current_text = String::new();

    loop {
        match reader.read_event() {
            Err(e) => {
                return Err(format!(
                    "XML error at position {}: {}",
                    reader.buffer_position(),
                    e
                ))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut obj = serde_json::Map::new();
                for attr in e.attributes().flatten() {
                    let key = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                    let val = String::from_utf8_lossy(&attr.value).to_string();
                    obj.insert(key, JsonValue::String(val));
                }
                stack.push((name, obj));
                current_text.clear();
            }
            Ok(Event::Text(e)) => {
                current_text.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::End(_)) => {
                if let Some((name, mut obj)) = stack.pop() {
                    let value = if obj.is_empty() && !current_text.is_empty() {
                        JsonValue::String(current_text.clone())
                    } else {
                        if !current_text.trim().is_empty() {
                            obj.insert("#text".into(), JsonValue::String(current_text.clone()));
                        }
                        JsonValue::Object(obj)
                    };
                    current_text.clear();

                    if let Some((_, parent)) = stack.last_mut() {
                        // Merge into parent: if key exists, convert to array
                        match parent.get_mut(&name) {
                            Some(existing) => {
                                if let JsonValue::Array(arr) = existing {
                                    arr.push(value);
                                } else {
                                    let old = existing.take();
                                    *existing = JsonValue::Array(vec![old, value]);
                                }
                            }
                            None => {
                                parent.insert(name, value);
                            }
                        }
                    } else {
                        root = Some((name, value));
                    }
                }
            }
            _ => {}
        }
    }

    match root {
        Some((name, value)) => {
            let mut obj = serde_json::Map::new();
            obj.insert(name, value);
            serde_json::to_string(&JsonValue::Object(obj))
                .map_err(|e| format!("JSON serialize failed: {}", e))
        }
        None => Err("No XML root element found".into()),
    }
}

// JSON → XML (basic: object keys become tags, arrays repeat)
fn json_to_xml(value: &JsonValue) -> Result<String, String> {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    match value {
        JsonValue::Object(map) if map.len() == 1 => {
            let (key, val) = map.iter().next().unwrap();
            write_xml_node(&mut out, key, val, 0);
        }
        _ => {
            write_xml_node(&mut out, "root", value, 0);
        }
    }
    Ok(out)
}

fn write_xml_node(out: &mut String, name: &str, value: &JsonValue, depth: usize) {
    let indent = "  ".repeat(depth);

    match value {
        JsonValue::Null => {
            out.push_str(&format!("{}<{}/>\n", indent, name));
        }
        JsonValue::Bool(b) => {
            out.push_str(&format!("{}<{}>{}</{}>\n", indent, name, b, name));
        }
        JsonValue::Number(n) => {
            out.push_str(&format!("{}<{}>{}</{}>\n", indent, name, n, name));
        }
        JsonValue::String(s) => {
            out.push_str(&format!(
                "{}<{}>{}</{}>\n",
                indent,
                name,
                escape_xml(s),
                name
            ));
        }
        JsonValue::Array(arr) => {
            for item in arr {
                write_xml_node(out, name, item, depth);
            }
        }
        JsonValue::Object(map) => {
            // Separate attributes (@-prefixed) and text (#text)
            let mut attrs = String::new();
            let mut text: Option<&str> = None;
            let mut children: Vec<(&String, &JsonValue)> = Vec::new();

            for (k, v) in map {
                if let Some(attr_name) = k.strip_prefix('@') {
                    if let JsonValue::String(s) = v {
                        attrs.push_str(&format!(" {}=\"{}\"", attr_name, escape_xml(s)));
                    }
                } else if k == "#text" {
                    if let JsonValue::String(s) = v {
                        text = Some(s);
                    }
                } else {
                    children.push((k, v));
                }
            }

            if children.is_empty() && text.is_some() {
                out.push_str(&format!(
                    "{}<{}{}>{}</{}>\n",
                    indent,
                    name,
                    attrs,
                    escape_xml(text.unwrap()),
                    name
                ));
            } else if children.is_empty() && text.is_none() {
                out.push_str(&format!("{}<{}{}/>\n", indent, name, attrs));
            } else {
                out.push_str(&format!("{}<{}{}>\n", indent, name, attrs));
                if let Some(t) = text {
                    out.push_str(&format!("{}  {}\n", indent, escape_xml(t)));
                }
                for (k, v) in children {
                    write_xml_node(out, k, v, depth + 1);
                }
                out.push_str(&format!("{}</{}>\n", indent, name));
            }
        }
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
