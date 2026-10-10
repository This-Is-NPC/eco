//! The devices every platform's audio lists: the system defaults first, then
//! each device found, inputs before outputs, by label.

use crate::ports::Device;

pub const DEFAULT_INPUT: &str = "@default-input";
pub const DEFAULT_OUTPUT: &str = "@default-output";

/// The system defaults, named in English as every daemon message is: the
/// overlay knows them by id and names them in the interface language.
pub fn defaults() -> Vec<Device> {
    vec![
        Device::new(DEFAULT_INPUT, "Default microphone", "input"),
        Device::new(DEFAULT_OUTPUT, "Default output", "output"),
    ]
}

/// The defaults, then `found` sorted by kind and label.
pub fn listed(mut found: Vec<Device>) -> Vec<Device> {
    found.sort_by_key(|device| (device.kind.clone(), device.label.to_lowercase()));
    let mut devices = defaults();
    devices.extend(found);
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_named_by_id_and_in_english() {
        let defaults = defaults();
        let named: Vec<(&str, &str)> = defaults
            .iter()
            .map(|device| (device.id.as_str(), device.label.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                ("@default-input", "Default microphone"),
                ("@default-output", "Default output")
            ]
        );
    }

    #[test]
    fn defaults_come_first_then_inputs_then_outputs_by_label() {
        let devices = listed(vec![
            Device::new("spk", "Speakers", "output"),
            Device::new("b", "built-in mic", "input"),
            Device::new("a", "Array Mic", "input"),
        ]);
        let mut expected = defaults();
        expected.extend([
            Device::new("a", "Array Mic", "input"),
            Device::new("b", "built-in mic", "input"),
            Device::new("spk", "Speakers", "output"),
        ]);
        assert_eq!(devices, expected);
    }
}
