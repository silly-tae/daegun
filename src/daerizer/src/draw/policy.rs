#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Prefer {
    #[default]
    Auto,
    Cpu,
    Gpu,
    Reference,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Policy {
    pub prefer: Prefer,
    // Off by default, and that is the safe direction: substituting draws a correct picture, while
    // refusing draws a hole. Strict is for a caller measuring which path ran.
    pub strict: bool,
    // A quality rule, not a performance guess. Small text is where hinting decides whether a stem
    // lands on a pixel boundary, and the GPU path has no hinting at all.
    pub cpu_below_ppem: Option<f32>,
    // On by default: WARP and Lavapipe report as devices, so a router that only asks "is there a
    // device" puts text on a CPU-implemented GPU – slower than daecpu *and* unhinted.
    pub avoid_software_gpu: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            prefer: Prefer::Auto,
            strict: false,
            cpu_below_ppem: Some(16.0),
            avoid_software_gpu: true,
        }
    }
}

impl Policy {
    pub fn prefer(prefer: Prefer) -> Policy {
        Policy { prefer, ..Policy::default() }
    }

    pub fn strictly(mut self) -> Policy {
        self.strict = true;
        self
    }

    pub fn at_any_size(mut self) -> Policy {
        self.cpu_below_ppem = None;
        self
    }
}
