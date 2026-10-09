//! What eco can listen to, read from `pw-dump`.

use serde_json::Value;
use tokio::process::Command;

use crate::adapters::echo_cancel::NODE_PREFIX;
use crate::ports::Device;

pub const DEFAULT_INPUT: &str = "@default-input";
pub const DEFAULT_OUTPUT: &str = "@default-output";

pub fn defaults() -> Vec<Device> {
    vec![
        Device::new(DEFAULT_INPUT, "Microfone padrão", "input"),
        Device::new(DEFAULT_OUTPUT, "Saída padrão", "output"),
    ]
}

fn kind_of(media_class: &str) -> Option<&'static str> {
    match media_class {
        "Audio/Source" => Some("input"),
        "Audio/Sink" => Some("output"),
        _ => None,
    }
}

/// Devices in `pw-dump` output, the system defaults first.
pub fn parse_dump(objects: &[Value]) -> Vec<Device> {
    let mut found: Vec<Device> = objects
        .iter()
        .filter_map(|item| {
            let props = item.get("info")?.get("props")?;
            let kind = kind_of(props.get("media.class")?.as_str()?)?;
            let name = props.get("node.name")?.as_str().filter(|n| !n.is_empty())?;
            // eco's own echo-cancelled microphone is not a device to choose.
            if name.starts_with(NODE_PREFIX) {
                return None;
            }
            let label = props
                .get("node.description")
                .and_then(Value::as_str)
                .filter(|l| !l.is_empty());
            Some(Device::new(name, label.unwrap_or(name), kind))
        })
        .collect();
    found.sort_by_key(|device| (device.kind.clone(), device.label.to_lowercase()));
    let mut devices = defaults();
    devices.extend(found);
    devices
}

pub async fn list_devices() -> Vec<Device> {
    let output = Command::new("pw-dump")
        .stderr(std::process::Stdio::null())
        .output()
        .await;
    let objects: Vec<Value> = output
        .ok()
        .and_then(|out| serde_json::from_slice(&out.stdout).ok())
        .unwrap_or_default();
    parse_dump(&objects)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn node(media_class: &str, name: Option<&str>, description: Option<&str>) -> Value {
        let mut props = json!({"media.class": media_class});
        if let Some(name) = name {
            props["node.name"] = json!(name);
        }
        if let Some(description) = description {
            props["node.description"] = json!(description);
        }
        json!({"info": {"props": props}})
    }

    #[test]
    fn lists_defaults_then_inputs_and_outputs() {
        let devices = parse_dump(&[
            node("Audio/Sink", Some("alsa_output.speaker"), Some("Speaker")),
            node(
                "Audio/Source",
                Some("alsa_input.mic"),
                Some("Digital Microphone"),
            ),
            node("Audio/Device", None, None),
            node("Stream/Output/Audio", Some("firefox"), None),
            node("Audio/Source", Some("alsa_input.headset"), None),
            node(
                "Audio/Source",
                Some("eco.aec.source.42"),
                Some("eco: echo cancelled"),
            ),
            json!({"type": "PipeWire:Interface:Link"}),
        ]);
        let mut expected = defaults();
        expected.extend([
            Device::new("alsa_input.headset", "alsa_input.headset", "input"),
            Device::new("alsa_input.mic", "Digital Microphone", "input"),
            Device::new("alsa_output.speaker", "Speaker", "output"),
        ]);
        assert_eq!(devices, expected);
    }
}
