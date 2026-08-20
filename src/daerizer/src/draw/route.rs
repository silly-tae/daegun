use super::device::DeviceProfile;
use super::policy::{Policy, Prefer};
use crate::daerizer::daegpu::GpuGlyphError;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rendered {
    Nothing,
    Cpu,
    Gpu,
    Reference,
    Scene,
    FlushAndRetry,
    Refused(Refusal),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    NonFinite,
    PreferenceUnmet,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Request {
    pub ppem: f32,
    pub hinted: bool,
    pub stroked: bool,
    pub gamma: bool,
    pub emboldened: bool,
    pub obliqued: bool,
}

impl Request {
    pub fn at(ppem: f32) -> Request {
        Request { ppem, ..Request::default() }
    }

    fn needs_cpu(&self) -> bool {
        self.hinted || self.stroked || self.gamma || self.emboldened || self.obliqued
    }
}

// Steps 1 to 4 below are outcomes no preference can change, and the order is load-bearing: a
// non-finite coordinate defeats every engine, and `NoOutline` is a correct result rather than a
// failure, so neither may be reached through a device or preference test.
pub fn route(
    attempt: Result<(), GpuGlyphError>,
    request: &Request,
    device: Option<&DeviceProfile>,
    policy: &Policy,
) -> Rendered {
    let gpu_eligible = match attempt {
        Ok(()) => true,
        Err(GpuGlyphError::NonFinite) => return Rendered::Refused(Refusal::NonFinite),
        Err(GpuGlyphError::NoOutline) => return Rendered::Nothing,
        Err(GpuGlyphError::BatchFull) => return Rendered::FlushAndRetry,
        Err(GpuGlyphError::NotFlatColor) => return Rendered::Scene,
        // No wildcard, deliberately. `GpuGlyphError` is `#[non_exhaustive]`, but that binds
        // downstream crates and this match is in-crate – so a new variant fails to compile here
        // and forces a routing decision, where `_ =>` would silently send it to the CPU.
        Err(GpuGlyphError::TooComplex) => false,
    };

    let gpu_possible = gpu_eligible && !request.needs_cpu();
    let device_usable = match device {
        Some(d) => !(policy.avoid_software_gpu && d.kind.is_software()),
        None => false,
    };
    let too_small = match policy.cpu_below_ppem {
        Some(limit) => request.ppem < limit,
        None => false,
    };

    match policy.prefer {
        Prefer::Cpu => Rendered::Cpu,
        // `eval` reads the batch's own packed buffers, so a glyph the batch would not take is one
        // it cannot evaluate either. It needs no device, which is the point of it.
        Prefer::Reference => {
            if gpu_eligible {
                Rendered::Reference
            } else {
                unmet(policy, Rendered::Cpu)
            }
        }
        Prefer::Gpu => {
            if gpu_possible && device_usable {
                Rendered::Gpu
            } else {
                unmet(policy, Rendered::Cpu)
            }
        }
        Prefer::Auto => {
            if gpu_possible && device_usable && !too_small {
                Rendered::Gpu
            } else {
                Rendered::Cpu
            }
        }
    }
}

fn unmet(policy: &Policy, fallback: Rendered) -> Rendered {
    if policy.strict {
        Rendered::Refused(Refusal::PreferenceUnmet)
    } else {
        fallback
    }
}
