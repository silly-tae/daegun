use daegun::daerizer::draw::{
    route, DeviceKind, DeviceProfile, Policy, Prefer, Refusal, Rendered, Request,
};
use daegun::daerizer::daegpu::GpuGlyphError;

type Case = (Result<(), GpuGlyphError>, Request, Option<DeviceProfile>, Policy);

fn every_case() -> Vec<Case> {
    let attempts = [
        Ok(()),
        Err(GpuGlyphError::TooComplex),
        Err(GpuGlyphError::NoOutline),
        Err(GpuGlyphError::NonFinite),
        Err(GpuGlyphError::BatchFull),
        Err(GpuGlyphError::NotFlatColor),
    ];
    let requests = [
        Request::at(8.0),
        Request::at(32.0),
        Request { ppem: 8.0, hinted: true, ..Request::default() },
        Request { ppem: 32.0, hinted: true, ..Request::default() },
        Request { ppem: 32.0, stroked: true, ..Request::default() },
        Request { ppem: 32.0, gamma: true, ..Request::default() },
        Request { ppem: 32.0, emboldened: true, ..Request::default() },
        Request { ppem: 32.0, obliqued: true, ..Request::default() },
    ];
    let devices = [
        None,
        Some(DeviceProfile::new(DeviceKind::Discrete, "discrete")),
        Some(DeviceProfile::new(DeviceKind::Integrated, "integrated")),
        Some(DeviceProfile::new(DeviceKind::Software, "warp")),
        Some(DeviceProfile::new(DeviceKind::Unknown, "unnamed")),
    ];
    let mut out = Vec::new();
    for attempt in attempts {
        for request in requests {
            for device in &devices {
                for prefer in [Prefer::Auto, Prefer::Cpu, Prefer::Gpu, Prefer::Reference] {
                    for strict in [false, true] {
                        for avoid in [false, true] {
                            for limit in [Some(16.0f32), None] {
                                out.push((
                                    attempt,
                                    request,
                                    device.clone(),
                                    Policy {
                                        prefer,
                                        strict,
                                        avoid_software_gpu: avoid,
                                        cpu_below_ppem: limit,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn the_whole_routing_table_holds_its_invariants() {
    let cases = every_case();
    assert_eq!(cases.len(), 7680, "the sweep no longer covers the space it claims to");

    let mut seen_gpu = 0usize;
    let mut seen_reference = 0usize;
    let mut seen_scene = 0usize;
    let mut seen_flush = 0usize;
    let mut seen_nothing = 0usize;
    let mut seen_refused = 0usize;

    for (attempt, request, device, policy) in &cases {
        let got = route(*attempt, request, device.as_ref(), policy);
        let what = format!("{attempt:?} {request:?} {device:?} {policy:?}");

        match attempt {
            Err(GpuGlyphError::NonFinite) => {
                assert_eq!(got, Rendered::Refused(Refusal::NonFinite), "not refused: {what}");
                seen_refused += 1;
                continue;
            }
            Err(GpuGlyphError::NoOutline) => {
                assert_eq!(got, Rendered::Nothing, "an absent outline became drawable: {what}");
                seen_nothing += 1;
                continue;
            }
            Err(GpuGlyphError::BatchFull) => {
                assert_eq!(got, Rendered::FlushAndRetry, "a full batch was not a flush: {what}");
                seen_flush += 1;
                continue;
            }
            Err(GpuGlyphError::NotFlatColor) => {
                assert_eq!(got, Rendered::Scene, "a colour scene was not routed as one: {what}");
                seen_scene += 1;
                continue;
            }
            _ => {}
        }

        let gpu_eligible = attempt.is_ok();
        let needs_cpu = request.hinted || request.stroked || request.gamma
            || request.emboldened || request.obliqued;
        let device_usable = device
            .as_ref()
            .is_some_and(|d| !(policy.avoid_software_gpu && d.kind == DeviceKind::Software));

        match got {
            Rendered::Gpu => {
                assert!(gpu_eligible, "a glyph the batch refused was routed to the GPU: {what}");
                assert!(!needs_cpu, "hinting, stroking or gamma went to the GPU: {what}");
                assert!(device_usable, "the GPU was chosen with no usable device: {what}");
                seen_gpu += 1;
            }
            Rendered::Reference => {
                assert!(gpu_eligible, "the reference path was given a glyph with no buffers: {what}");
                assert_eq!(policy.prefer, Prefer::Reference, "the reference path was not asked for: {what}");
                seen_reference += 1;
            }
            Rendered::Refused(Refusal::PreferenceUnmet) => {
                assert!(policy.strict, "a preference was refused without strict: {what}");
                seen_refused += 1;
            }
            Rendered::Cpu => {}
            other => panic!("unreachable outcome {other:?} for {what}"),
        }

        if policy.prefer == Prefer::Auto {
            assert!(
                matches!(got, Rendered::Cpu | Rendered::Gpu),
                "Auto produced {got:?}, which is neither engine: {what}",
            );
        }
        if policy.prefer == Prefer::Cpu {
            assert_eq!(got, Rendered::Cpu, "a CPU preference was not honoured: {what}");
        }
    }

    for (n, name) in [
        (seen_gpu, "Gpu"), (seen_reference, "Reference"), (seen_scene, "Scene"),
        (seen_flush, "FlushAndRetry"), (seen_nothing, "Nothing"), (seen_refused, "Refused"),
    ] {
        assert!(n > 0, "{name} never occurred in the sweep, so its invariant proved nothing");
    }
}

#[test]
fn the_outcomes_the_gpu_error_documentation_names() {
    let plain = Request::at(32.0);
    let gpu = DeviceProfile::new(DeviceKind::Discrete, "gpu");
    let auto = Policy::default();

    assert_eq!(
        route(Err(GpuGlyphError::TooComplex), &plain, Some(&gpu), &auto),
        Rendered::Cpu,
        "\"TooComplex means draw it on the CPU instead\"",
    );
    assert_eq!(
        route(Err(GpuGlyphError::NoOutline), &plain, Some(&gpu), &auto),
        Rendered::Nothing,
        "\"NoOutline means draw nothing\"",
    );
    assert_eq!(
        route(Err(GpuGlyphError::NotFlatColor), &plain, Some(&gpu), &auto),
        Rendered::Scene,
        "\"not a failure ... render_colr_glyph draws it on the CPU\"",
    );
    assert_eq!(
        route(Err(GpuGlyphError::BatchFull), &plain, Some(&gpu), &auto),
        Rendered::FlushAndRetry,
        "\"draw what it holds, then start a new one\"",
    );
    assert_eq!(
        route(Err(GpuGlyphError::NonFinite), &plain, Some(&gpu), &auto),
        Rendered::Refused(Refusal::NonFinite),
        "\"not drawable by either path\"",
    );
    assert_eq!(route(Ok(()), &plain, Some(&gpu), &auto), Rendered::Gpu, "a plain glyph at 32px");
}

#[test]
fn a_software_gpu_is_not_a_gpu_worth_routing_to() {
    let plain = Request::at(32.0);
    let warp = DeviceProfile::new(DeviceKind::Software, "Microsoft Basic Render Driver");
    assert_eq!(
        route(Ok(()), &plain, Some(&warp), &Policy::default()),
        Rendered::Cpu,
        "a software rasterizer was treated as a GPU",
    );
    let allow = Policy { avoid_software_gpu: false, ..Policy::default() };
    assert_eq!(
        route(Ok(()), &plain, Some(&warp), &allow),
        Rendered::Gpu,
        "a caller who opted into a software device did not get it",
    );

    assert_eq!(DeviceKind::from_vulkan(4), DeviceKind::Software, "VK_PHYSICAL_DEVICE_TYPE_CPU");
    assert_eq!(DeviceKind::from_vulkan(2), DeviceKind::Discrete, "VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU");
    assert_eq!(DeviceKind::from_vulkan(1), DeviceKind::Integrated, "VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU");
    assert_eq!(DeviceKind::from_vulkan(3), DeviceKind::Virtual, "VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU");
    assert_eq!(DeviceKind::from_vulkan(0), DeviceKind::Unknown, "VK_PHYSICAL_DEVICE_TYPE_OTHER");
    assert_eq!(DeviceKind::from_vulkan(99), DeviceKind::Unknown, "an unknown device type");
    assert!(!DeviceKind::Unknown.is_software(), "an unnamed device was treated as software");
}

#[test]
fn the_cpu_only_features_route_themselves() {
    let gpu = DeviceProfile::new(DeviceKind::Discrete, "gpu");
    for req in [
        Request { ppem: 64.0, hinted: true, ..Request::default() },
        Request { ppem: 64.0, stroked: true, ..Request::default() },
        Request { ppem: 64.0, gamma: true, ..Request::default() },
        Request { ppem: 64.0, emboldened: true, ..Request::default() },
        Request { ppem: 64.0, obliqued: true, ..Request::default() },
    ] {
        assert_eq!(
            route(Ok(()), &req, Some(&gpu), &Policy::default()),
            Rendered::Cpu,
            "{req:?} was routed to a path that cannot do it",
        );
        assert_eq!(
            route(Ok(()), &req, Some(&gpu), &Policy::prefer(Prefer::Gpu).strictly()),
            Rendered::Refused(Refusal::PreferenceUnmet),
            "{req:?} under a strict GPU preference was substituted instead of refused",
        );
    }
}

#[test]
fn small_text_goes_where_the_hinting_is() {
    let gpu = DeviceProfile::new(DeviceKind::Discrete, "gpu");
    let auto = Policy::default();
    assert_eq!(route(Ok(()), &Request::at(8.0), Some(&gpu), &auto), Rendered::Cpu, "8px");
    assert_eq!(route(Ok(()), &Request::at(15.9), Some(&gpu), &auto), Rendered::Cpu, "just under");
    assert_eq!(route(Ok(()), &Request::at(16.0), Some(&gpu), &auto), Rendered::Gpu, "exactly at");
    assert_eq!(route(Ok(()), &Request::at(64.0), Some(&gpu), &auto), Rendered::Gpu, "64px");
    let any = Policy::default().at_any_size();
    assert_eq!(route(Ok(()), &Request::at(8.0), Some(&gpu), &any), Rendered::Gpu, "rule disabled");
}

#[test]
fn the_reference_path_needs_buffers_but_not_a_device() {
    let plain = Request::at(32.0);
    let want = Policy::prefer(Prefer::Reference);
    assert_eq!(route(Ok(()), &plain, None, &want), Rendered::Reference, "with no device");
    assert_eq!(
        route(Err(GpuGlyphError::TooComplex), &plain, None, &want),
        Rendered::Cpu,
        "the reference path was handed a glyph with no buffers",
    );
    assert_eq!(
        route(Err(GpuGlyphError::TooComplex), &plain, None, &want.strictly()),
        Rendered::Refused(Refusal::PreferenceUnmet),
        "strict mode substituted instead of refusing",
    );
}
