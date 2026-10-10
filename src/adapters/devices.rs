//! The devices every platform's audio lists: the system defaults first, then
//! each device found, inputs before outputs, by label.

use crate::ports::Device;

pub const DEFAULT_INPUT: &str = "@default-input";
pub const DEFAULT_OUTPUT: &str = "@default-output";

pub fn defaults() -> Vec<Device> {
    vec![
        Device::new(DEFAULT_INPUT, "Microfone padrão", "input"),
        Device::new(DEFAULT_OUTPUT, "Saída padrão", "output"),
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
