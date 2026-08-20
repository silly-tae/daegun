#include <metal_stdlib>

#if __METAL_VERSION__ < 220
#error "daegun needs Metal Shading Language 2.2 or later (macOS 10.15 / iOS 13)."
#endif

using namespace metal;

struct GlyphInstance {
    float4 glyphBox;
    float4 tint;
    float2 offset;
    float2 emPixels;
    float  scale;
    uint   bandBase;
    uint   bandsPerAxis;
    uint   hullBase;
    float  invScale;
    // Three scalars and not a 3-vector: a three-component vector aligns to 16, which puts it at
    // offset 80 and makes the struct 96 while Rust says 80 – every instance after the first then
    // reads from the wrong stride.
    float  _pad0;
    float  _pad1;
    float  _pad2;
};

struct HullVertex {
    float2 pos;
    float2 dilateX;
    float2 dilateY;
};

struct SubpixelParams {
    float weights[192];
    uint2 oversample;
    uint2 taps;
    int2  origin;
    uint  channels;
    uint  supersample;
};

struct Band {
    uint firstCurve;
    uint curveCount;
};

struct DrawUniform {
    float4x4 projection;
    float4   view;
};

struct FragmentInput {
    float4 position [[position]];
    float2 emCoord;
    uint   bandBase  [[flat]];
    uint   bands     [[flat]];
    float4 glyphBox  [[flat]];
    float4 glyphTint [[flat]];
    float2 emPixels  [[flat]];
    float4 emAnchor  [[flat]];
    float  emExact   [[flat]];
};

#ifdef DAEGUN_VERTEX

vertex FragmentInput daegunGlyphVertex(
    uint vertexID   [[vertex_id]],
    uint instanceID [[instance_id]],
    device const GlyphInstance *instances [[buffer(3)]],
    constant SubpixelParams &subpixel [[buffer(4)]],
    device const HullVertex *hull [[buffer(5)]],
    constant DrawUniform &uDraw [[buffer(6)]])
{
    GlyphInstance glyph = instances[instanceID];
    HullVertex corner = hull[glyph.hullBase + vertexID];

    float2 dilate = float2(0.5, 0.5);
    if (subpixel.origin.x < 0 && subpixel.oversample.x > 0u) {
        dilate.x = max(0.5f, ceil(float(-subpixel.origin.x) / float(subpixel.oversample.x)));
    }
    if (subpixel.origin.y < 0 && subpixel.oversample.y > 0u) {
        dilate.y = max(0.5f, ceil(float(-subpixel.origin.y) / float(subpixel.oversample.y)));
    }
    float2 pad = dilate / max(glyph.emPixels, float2(1.0e-6));

    FragmentInput out;
    out.bandBase  = glyph.bandBase;
    out.bands     = glyph.bandsPerAxis;
    out.glyphBox  = glyph.glyphBox;
    out.glyphTint = glyph.tint;
    out.emPixels  = glyph.emPixels;
    out.emCoord   = corner.pos + float2(dot(corner.dilateX, pad), dot(corner.dilateY, pad));
    out.position  = uDraw.projection * float4(glyph.offset + out.emCoord * glyph.scale, 0.0, 1.0);
    out.emAnchor  = float4(glyph.offset.x, uDraw.view.x - glyph.offset.y, glyph.invScale, glyph.scale);
    out.emExact   = uDraw.view.y;
    return out;
}

#endif

#if defined(DAEGUN_FRAGMENT) || defined(DAEGUN_SUBPIXEL)

static float2 localScale(float2 emCoord, float2 emPixels);

static float2 localScaleOf(FragmentInput in, float2 emCoord) {
    if (in.emExact != 0.0f) {
        return float2(in.emAnchor.w, in.emAnchor.w);
    }
    return localScale(emCoord, in.emPixels);
}

static float2 emCoordOf(FragmentInput in) {
    if (in.emExact == 0.0f) {
        return in.emCoord;
    }
    return float2((in.position.x - in.emAnchor.x) * in.emAnchor.z,
                  (in.emAnchor.y - in.position.y) * in.emAnchor.z);
}

static float2 localScale(float2 emCoord, float2 emPixels) {
    float2 dx = dfdx(emCoord);
    float2 dy = dfdy(emCoord);
    float det = dx.x * dy.y - dy.x * dx.y;
    if (abs(det) < 1.0e-20) {
        return emPixels;
    }
    float inv = 1.0 / abs(det);
    return float2(length(float2(dy.y, -dx.y)), length(float2(-dy.x, dx.x))) * inv;
}

static uint rootContribution(float y1, float y2, float y3) {
    uint signCode = (y1 > 0.0 ? 2u : 0u) + (y2 > 0.0 ? 4u : 0u) + (y3 > 0.0 ? 8u : 0u);
    return (0x2E74u >> signCode) & 3u;
}

static float curveX(float2 p1, float2 p2, float2 p3, float t) {
    float u = 1.0 - t;
    return u * u * p1.x + 2.0 * t * u * p2.x + t * t * p3.x;
}

static float3 curveRoots(float2 p1, float2 p2, float2 p3) {
    uint contribution = rootContribution(p1.y, p2.y, p3.y);
    if (contribution == 0u) {
        return float3(0.0, 0.0, 0.0);
    }

    float a = p1.y - 2.0 * p2.y + p3.y;
    float b = p1.y - p2.y;
    float c = p1.y;

    float t1, t2;
    if (abs(a) < 1.0e-5) {
        t1 = t2 = (b != 0.0) ? c / (2.0 * b) : 0.0;
    } else {
        float root = sqrt(max(b * b - a * c, 0.0));
        t1 = (b - root) / a;
        t2 = (b + root) / a;
    }
    return float3(float(contribution), t1, t2);
}

static float2 curveCoverageAt(float2 p1, float2 p2, float2 p3, float3 roots, float pixelsPerEm) {
    uint contribution = uint(roots.x);
    float coverage = 0.0;
    float weight = 0.0;
    if ((contribution & 1u) != 0u) {
        float x = pixelsPerEm * curveX(p1, p2, p3, roots.y);
        coverage += saturate(x + 0.5);
        weight = max(weight, saturate(1.0 - 2.0 * abs(x)));
    }
    if ((contribution & 2u) != 0u) {
        float x = pixelsPerEm * curveX(p1, p2, p3, roots.z);
        coverage -= saturate(x + 0.5);
        weight = max(weight, saturate(1.0 - 2.0 * abs(x)));
    }
    return float2(coverage, weight);
}

static float combineRays(float2 h, float2 v) {
    float floorCov = min(abs(h.x), abs(v.x));
    return max(abs(h.y >= v.y ? h.x : v.x), floorCov);
}

static float2 curveCoverage(float2 p1, float2 p2, float2 p3, float pixelsPerEm) {
    float3 roots = curveRoots(p1, p2, p3);
    if (roots.x == 0.0) {
        return float2(0.0, 0.0);
    }
    return curveCoverageAt(p1, p2, p3, roots, pixelsPerEm);
}

static uint bandIndex(float t, float lo, float hi, uint count) {
    // A count of zero makes `float(count) - 1.0` negative, crossing the clamp's own bounds –
    // undefined, and in practice -1.0, which `uint()` turns into 0xFFFFFFFF. Reachable from a
    // default GlyphSlot, the natural placeholder for a glyph the GPU path refused.
    if (count == 0u) {
        return 0u;
    }
    float span = max(hi - lo, 1.0e-6);
    float f = (t - lo) / span * float(count);
    return uint(clamp(f, 0.0, float(count) - 1.0));
}

static float2 scanBand(uint band,
                      float2 origin,
                      float pixelsPerEm,
                      bool transpose,
                      device const float2 *curvePoints,
                      device const uint *bandCurves,
                      device const Band *bands) {
    Band slice = bands[band];
    float coverage = 0.0;
    float weight = 0.0;

    for (uint i = 0u; i < slice.curveCount; ++i) {
        uint base = bandCurves[slice.firstCurve + i] * 3u;

        float2 p1 = curvePoints[base]      - origin;
        float2 p2 = curvePoints[base + 1u] - origin;
        float2 p3 = curvePoints[base + 2u] - origin;

        // A rotation, not an axis swap: swapping mirrors the plane, a mirror reverses every winding
        // sign, and the two rays would then cancel to zero everywhere instead of agreeing.
        if (transpose) {
            p1 = float2(p1.y, -p1.x);
            p2 = float2(p2.y, -p2.x);
            p3 = float2(p3.y, -p3.x);
        }

        if (max(max(p1.x, p2.x), p3.x) * pixelsPerEm < -0.5) {
            break;
        }

        float2 c = curveCoverage(p1, p2, p3, pixelsPerEm);
        coverage += c.x;
        weight = max(weight, c.y);
    }

    return float2(coverage, weight);
}

static float windingAt(FragmentInput in,
                       float2 at,
                       float2 m,
                       device const float2 *curvePoints,
                       device const uint *bandCurves,
                       device const Band *bands) {
    uint hBand = in.bandBase + bandIndex(at.y, in.glyphBox.y, in.glyphBox.w, in.bands);
    if (hBand < in.bandBase) { return 0.0; }
    uint vBand = in.bandBase + in.bands
               + bandIndex(at.x, in.glyphBox.x, in.glyphBox.z, in.bands);
    if (vBand < in.bandBase) { return 0.0; }

    return combineRays(scanBand(hBand, at, m.x, false, curvePoints, bandCurves, bands),
                       scanBand(vBand, at, m.y, true, curvePoints, bandCurves, bands));
}

#ifdef DAEGUN_FRAGMENT

fragment float4 daegunGlyphFragment(
    FragmentInput in [[stage_in]],
    device const float2 *curvePoints [[buffer(0)]],
    device const uint   *bandCurves  [[buffer(1)]],
    device const Band   *bands       [[buffer(2)]])
{
    float2 emc = emCoordOf(in);
    float coverage = windingAt(in, emc, localScaleOf(in, emc),
                               curvePoints, bandCurves, bands);

    return float4(in.glyphTint.rgb, in.glyphTint.a * saturate(abs(coverage)));
}

#endif

#ifdef DAEGUN_SUBPIXEL

struct FragmentOutput {
    float4 color   [[color(0), index(0)]];
    float4 coverage [[color(0), index(1)]];
};

static float4 scanBandRow4(uint band,
                           float4 xs,
                           float y,
                           float pixelsPerEm,
                           thread float4 &weightOut,
                           device const float2 *curvePoints,
                           device const uint *bandCurves,
                           device const Band *bands) {
    Band slice = bands[band];
    float4 coverage = float4(0.0, 0.0, 0.0, 0.0);
    float4 weight = float4(0.0, 0.0, 0.0, 0.0);
    float minX = min(min(xs.x, xs.y), min(xs.z, xs.w));

    for (uint i = 0u; i < slice.curveCount; ++i) {
        uint base = bandCurves[slice.firstCurve + i] * 3u;
        float2 q1 = curvePoints[base];
        float2 q2 = curvePoints[base + 1u];
        float2 q3 = curvePoints[base + 2u];

        if ((max(max(q1.x, q2.x), q3.x) - minX) * pixelsPerEm < -0.5) {
            break;
        }

        float e1 = q1.y - y;
        float e2 = q2.y - y;
        float e3 = q3.y - y;
        float3 roots = curveRoots(float2(0.0, e1), float2(0.0, e2), float2(0.0, e3));
        if (roots.x == 0.0) {
            continue;
        }

        uint contribution = uint(roots.x);
        if ((contribution & 1u) != 0u) {
            float4 x = pixelsPerEm * (curveX(q1, q2, q3, roots.y) - xs);
            coverage += saturate(x + 0.5);
            weight = max(weight, saturate(1.0 - 2.0 * abs(x)));
        }
        if ((contribution & 2u) != 0u) {
            float4 x = pixelsPerEm * (curveX(q1, q2, q3, roots.z) - xs);
            coverage -= saturate(x + 0.5);
            weight = max(weight, saturate(1.0 - 2.0 * abs(x)));
        }
    }
    weightOut = weight;
    return coverage;
}

static float4 windingRow4(FragmentInput in,
                          float4 xs,
                          float y,
                          float2 m,
                          device const float2 *curvePoints,
                          device const uint *bandCurves,
                          device const Band *bands) {
    uint hBand = in.bandBase + bandIndex(y, in.glyphBox.y, in.glyphBox.w, in.bands);
    if (hBand < in.bandBase) { return float4(0.0, 0.0, 0.0, 0.0); }

    float4 hw;
    float4 h = scanBandRow4(hBand, xs, y, m.x, hw, curvePoints, bandCurves, bands);

    float4 v = float4(0.0, 0.0, 0.0, 0.0);
    float4 vw = float4(0.0, 0.0, 0.0, 0.0);
    uint vBase = in.bandBase + in.bands;
    if (vBase >= in.bandBase) {
        float2 r;
        r = scanBand(vBase + bandIndex(xs.x, in.glyphBox.x, in.glyphBox.z, in.bands),
                     float2(xs.x, y), m.y, true, curvePoints, bandCurves, bands);
        v.x = r.x; vw.x = r.y;
        r = scanBand(vBase + bandIndex(xs.y, in.glyphBox.x, in.glyphBox.z, in.bands),
                     float2(xs.y, y), m.y, true, curvePoints, bandCurves, bands);
        v.y = r.x; vw.y = r.y;
        r = scanBand(vBase + bandIndex(xs.z, in.glyphBox.x, in.glyphBox.z, in.bands),
                     float2(xs.z, y), m.y, true, curvePoints, bandCurves, bands);
        v.z = r.x; vw.z = r.y;
        r = scanBand(vBase + bandIndex(xs.w, in.glyphBox.x, in.glyphBox.z, in.bands),
                     float2(xs.w, y), m.y, true, curvePoints, bandCurves, bands);
        v.w = r.x; vw.w = r.y;
    }

    float4 floorCov = min(abs(h), abs(v));
    return max(abs(select(v, h, hw >= vw)), floorCov);
}

static float jitter(ushort i, uint ss, float m) {
    if (ss == 1u || m == 0.0) {
        return 0.0;
    }
    return ((float(i) + 0.5) / float(ss) - 0.5) / m;
}

static float tapOffset(ushort tap, int origin, uint oversample, float pixelsPerEm) {
    if (oversample == 0u || pixelsPerEm == 0.0) {
        return 0.0;
    }
    float centre = (float(origin) + float(tap) + 0.5) / float(oversample);
    return (centre - 0.5) / pixelsPerEm;
}

fragment FragmentOutput daegunGlyphSubpixelFragment(
    FragmentInput in [[stage_in]],
    device const float2 *curvePoints [[buffer(0)]],
    device const uint   *bandCurves  [[buffer(1)]],
    device const Band   *bands       [[buffer(2)]],
    constant SubpixelParams &subpixel [[buffer(4)]])
{
    float2 emc = emCoordOf(in);
    float2 m = localScaleOf(in, emc);
    uint2 os   = max(subpixel.oversample, uint2(1u, 1u));
    uint2 taps = min(max(subpixel.taps, uint2(1u, 1u)), uint2(8u, 8u));

    uint  ss = min(max(subpixel.supersample, 1u), 4u);
    float invSS = 1.0 / float(ss * ss);

    float2 sampleM = m * float2(os);

    float3 channels = float3(0.0, 0.0, 0.0);
    for (ushort ty = 0u; ty < ushort(taps.y); ++ty) {
        float dy = -tapOffset(ty, subpixel.origin.y, os.y, m.y);
        for (ushort sy = 0u; sy < ushort(ss); ++sy) {
            float y = emc.y + dy + jitter(sy, ss, sampleM.y);

            ushort width = ushort(taps.x) * ushort(ss);
            for (ushort k = 0u; k < width; k += 4u) {
                float4 xs;
                float4 wr = float4(0.0, 0.0, 0.0, 0.0);
                float4 wg = float4(0.0, 0.0, 0.0, 0.0);
                float4 wb = float4(0.0, 0.0, 0.0, 0.0);
                for (ushort lane = 0u; lane < 4u; ++lane) {
                    ushort at = min(ushort(k + lane), ushort(width - 1u));
                    ushort tx = at;
                    ushort sx = 0u;
                    if (ss != 1u) {
                        tx = at / ushort(ss);
                        sx = at - tx * ushort(ss);
                    }
                    float x = emc.x
                            + tapOffset(tx, subpixel.origin.x, os.x, m.x)
                            + jitter(sx, ss, sampleM.x);
                    float live = (k + lane < width) ? 1.0 : 0.0;
                    uint idx = uint(ty) * taps.x + uint(tx);
                    xs[lane] = x;
                    wr[lane] = live * subpixel.weights[idx];
                    wg[lane] = live * subpixel.weights[64u + idx];
                    wb[lane] = live * subpixel.weights[128u + idx];
                }

                float4 cov = saturate(abs(windingRow4(in, xs, y, sampleM,
                                                      curvePoints, bandCurves, bands))) * invSS;
                channels.r += dot(wr, cov);
                channels.g += dot(wg, cov);
                channels.b += dot(wb, cov);
            }
        }
    }

    if (subpixel.channels < 2u) {
        channels = float3(channels.r, channels.r, channels.r);
    }
    channels = saturate(channels);

    FragmentOutput out;
    out.color   = float4(in.glyphTint.rgb, 1.0);
    out.coverage = float4(in.glyphTint.a * channels,
                          in.glyphTint.a * max(channels.r, max(channels.g, channels.b)));
    return out;
}

#endif

#endif
