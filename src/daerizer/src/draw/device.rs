use alloc::string::String;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeviceKind {
    #[default]
    // Routes as real hardware: refusing a device that works is worse than using a slow one.
    Unknown,
    Discrete,
    Integrated,
    Virtual,
    // A conformant GPU implementation running on the host's cores – WARP passes WHQL, Lavapipe
    // passes Vulkan CTS – so it renders correctly. It is just slower than daecpu, and cannot hint.
    Software,
}

impl DeviceKind {
    pub fn from_vulkan(device_type: i32) -> DeviceKind {
        match device_type {
            1 => DeviceKind::Integrated,
            2 => DeviceKind::Discrete,
            3 => DeviceKind::Virtual,
            4 => DeviceKind::Software,
            _ => DeviceKind::Unknown,
        }
    }

    pub fn is_software(self) -> bool {
        matches!(self, DeviceKind::Software)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DeviceProfile {
    pub kind: DeviceKind,
    pub name: String,
}

impl DeviceProfile {
    pub fn new(kind: DeviceKind, name: impl Into<String>) -> DeviceProfile {
        DeviceProfile { kind, name: name.into() }
    }

    pub fn from_vulkan(device_type: i32, name: impl Into<String>) -> DeviceProfile {
        DeviceProfile::new(DeviceKind::from_vulkan(device_type), name)
    }

    // Unified memory is what integrated *means*, and both APIs state it outright. Dedicated video
    // memory is the usual guess and only a guess: a UMA part can report a carve-out.
    pub fn from_d3d(software: bool, uma: Option<bool>, name: impl Into<String>) -> DeviceProfile {
        let kind = match (software, uma) {
            (true, _) => DeviceKind::Software,
            (false, Some(true)) => DeviceKind::Integrated,
            (false, Some(false)) => DeviceKind::Discrete,
            (false, None) => DeviceKind::Unknown,
        };
        DeviceProfile::new(kind, name)
    }

    pub fn from_metal(uma: Option<bool>, name: impl Into<String>) -> DeviceProfile {
        let kind = match uma {
            Some(true) => DeviceKind::Integrated,
            Some(false) => DeviceKind::Discrete,
            None => DeviceKind::Unknown,
        };
        DeviceProfile::new(kind, name)
    }
}
