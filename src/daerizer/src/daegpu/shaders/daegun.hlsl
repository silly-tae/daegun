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

StructuredBuffer<GlyphInstance> instances : register(t3);

struct HullVertex {
    float2 pos;
    float2 dilateX;
    float2 dilateY;
};

StructuredBuffer<HullVertex> hull : register(t5);

struct SubpixelParams {
    float weights[192];
    uint2 oversample;
    uint2 taps;
    int2  origin;
    uint  channels;
    uint  supersample;
};
StructuredBuffer<SubpixelParams> subpixel : register(t4);

cbuffer Projection : register(b0) {
    float4x4 uProjection;
    float4   uView;
};

StructuredBuffer<float2> curvePoints : register(t0);

StructuredBuffer<uint> bandCurves : register(t1);

StructuredBuffer<uint2> bands : register(t2);

struct PixelInput {
    float4 position  : SV_Position;
    float2 emCoord   : TEXCOORD0;
    nointerpolation float4 emAnchor : TEXCOORD6;
    nointerpolation float  emExact  : TEXCOORD7;
    nointerpolation uint  bandBase : TEXCOORD1;
    nointerpolation uint bands : TEXCOORD2;
    nointerpolation float4 glyphBox : TEXCOORD3;
    nointerpolation float4 glyphTint : TEXCOORD4;
    nointerpolation float2 emPixels : TEXCOORD5;
};

#ifdef DAEGUN_VERTEX

float2 dilationPixels() {
    float2 d = float2(0.5, 0.5);
    if (subpixel[0].origin.x < 0 && subpixel[0].oversample.x > 0u) {
        d.x = max(0.5, ceil((float)(-subpixel[0].origin.x) / (float)subpixel[0].oversample.x));
    }
    if (subpixel[0].origin.y < 0 && subpixel[0].oversample.y > 0u) {
        d.y = max(0.5, ceil((float)(-subpixel[0].origin.y) / (float)subpixel[0].oversample.y));
    }
    return d;
}

PixelInput main(uint vertexID : SV_VertexID, uint instanceID : SV_InstanceID) {
    GlyphInstance glyph = instances[instanceID];
    HullVertex corner = hull[glyph.hullBase + vertexID];

    float2 pad = dilationPixels() / max(glyph.emPixels, float2(1.0e-6, 1.0e-6));

    PixelInput output;
    output.emCoord   = corner.pos + float2(dot(corner.dilateX, pad), dot(corner.dilateY, pad));
    output.bandBase  = glyph.bandBase;
    output.bands     = glyph.bandsPerAxis;
    output.glyphBox  = glyph.glyphBox;
    output.glyphTint = glyph.tint;
    output.emPixels  = glyph.emPixels;
    output.position  = mul(uProjection,
                           float4(glyph.offset + output.emCoord * glyph.scale, 0.0, 1.0));
    output.emAnchor  = float4(glyph.offset.x, uView.x - glyph.offset.y, glyph.invScale, glyph.scale);
    output.emExact   = uView.y;
    return output;
}

#endif

#if defined(DAEGUN_FRAGMENT) || defined(DAEGUN_SUBPIXEL)

float2 emCoordOf(PixelInput input) {
    if (input.emExact == 0.0) {
        return input.emCoord;
    }
    return float2((input.position.x - input.emAnchor.x) * input.emAnchor.z,
                  (input.emAnchor.y - input.position.y) * input.emAnchor.z);
}

float2 localScale(float2 emCoord, float2 emPixels);

float2 localScaleOf(PixelInput input, float2 emCoord) {
    if (input.emExact != 0.0) {
        return float2(input.emAnchor.w, input.emAnchor.w);
    }
    return localScale(emCoord, input.emPixels);
}

float2 localScale(float2 emCoord, float2 emPixels) {
    float2 dx = ddx(emCoord);
    float2 dy = ddy(emCoord);
    float det = dx.x * dy.y - dy.x * dx.y;
    if (abs(det) < 1.0e-20) {
        return emPixels;
    }
    float inv = 1.0 / abs(det);
    return float2(length(float2(dy.y, -dx.y)), length(float2(-dy.x, dx.x))) * inv;
}

uint rootContribution(float y1, float y2, float y3) {
    uint signCode = (y1 > 0.0 ? 2u : 0u) + (y2 > 0.0 ? 4u : 0u) + (y3 > 0.0 ? 8u : 0u);
    return (0x2E74u >> signCode) & 3u;
}

float curveX(float2 p1, float2 p2, float2 p3, float t) {
    float u = 1.0 - t;
    return u * u * p1.x + 2.0 * t * u * p2.x + t * t * p3.x;
}

float3 curveRoots(float2 p1, float2 p2, float2 p3) {
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
    return float3((float)contribution, t1, t2);
}

float2 curveCoverageAt(float2 p1, float2 p2, float2 p3, float3 roots, float pixelsPerEm) {
    uint contribution = (uint)roots.x;
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

float combineRays(float2 h, float2 v) {
    float floorCov = min(abs(h.x), abs(v.x));
    return max(abs(h.y >= v.y ? h.x : v.x), floorCov);
}

float2 curveCoverage(float2 p1, float2 p2, float2 p3, float pixelsPerEm) {
    float3 roots = curveRoots(p1, p2, p3);
    if (roots.x == 0.0) {
        return float2(0.0, 0.0);
    }
    return curveCoverageAt(p1, p2, p3, roots, pixelsPerEm);
}

uint bandIndex(float t, float lo, float hi, uint count) {
    // A count of zero makes `float(count) - 1.0` negative, crossing the clamp's own bounds –
    // undefined, and in practice -1.0, which `uint()` turns into 0xFFFFFFFF. Reachable from a
    // default GlyphSlot, the natural placeholder for a glyph the GPU path refused.
    if (count == 0u) {
        return 0u;
    }
    float span = max(hi - lo, 1.0e-6);
    float f = (t - lo) / span * (float)count;
    return (uint)clamp(f, 0.0, (float)count - 1.0);
}

float2 scanBand(uint band, float2 origin, float pixelsPerEm, bool transpose) {
    uint2 slice = bands[band];
    float coverage = 0.0;
    float weight = 0.0;

    for (uint i = 0u; i < slice.y; ++i) {
        uint base = bandCurves[slice.x + i] * 3u;

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

float windingAt(PixelInput input, float2 at, float2 m) {
    uint hBand = input.bandBase
               + bandIndex(at.y, input.glyphBox.y, input.glyphBox.w, input.bands);
    if (hBand < input.bandBase) { return 0.0; }
    uint vBand = input.bandBase + input.bands
               + bandIndex(at.x, input.glyphBox.x, input.glyphBox.z, input.bands);
    if (vBand < input.bandBase) { return 0.0; }

    return combineRays(scanBand(hBand, at, m.x, false), scanBand(vBand, at, m.y, true));
}

#ifdef DAEGUN_FRAGMENT

float4 main(PixelInput input) : SV_Target {
    float2 emCoord = emCoordOf(input);
    precise float coverage = windingAt(input, emCoord, localScaleOf(input, emCoord));

    return float4(input.glyphTint.rgb, input.glyphTint.a * saturate(abs(coverage)));
}

#endif

#ifdef DAEGUN_SUBPIXEL

struct PixelOutput {
    float4 color   : SV_Target0;
    float4 coverage : SV_Target1;
};

float4 scanBandRow4(uint band, float4 xs, float y, float pixelsPerEm, out float4 weightOut) {
    uint2 slice = bands[band];
    float4 coverage = float4(0.0, 0.0, 0.0, 0.0);
    float4 weight = float4(0.0, 0.0, 0.0, 0.0);
    float minX = min(min(xs.x, xs.y), min(xs.z, xs.w));

    for (uint i = 0u; i < slice.y; ++i) {
        uint base = bandCurves[slice.x + i] * 3u;
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

float4 windingRow4(PixelInput input, float4 xs, float y, float2 m) {
    uint hBand = input.bandBase + bandIndex(y, input.glyphBox.y, input.glyphBox.w, input.bands);
    if (hBand < input.bandBase) { return float4(0.0, 0.0, 0.0, 0.0); }

    float4 hw;
    float4 h = scanBandRow4(hBand, xs, y, m.x, hw);

    float4 v = float4(0.0, 0.0, 0.0, 0.0);
    float4 vw = float4(0.0, 0.0, 0.0, 0.0);
    uint vBase = input.bandBase + input.bands;
    if (vBase >= input.bandBase) {
        float2 r;
        r = scanBand(vBase + bandIndex(xs.x, input.glyphBox.x, input.glyphBox.z, input.bands),
                     float2(xs.x, y), m.y, true); v.x = r.x; vw.x = r.y;
        r = scanBand(vBase + bandIndex(xs.y, input.glyphBox.x, input.glyphBox.z, input.bands),
                     float2(xs.y, y), m.y, true); v.y = r.x; vw.y = r.y;
        r = scanBand(vBase + bandIndex(xs.z, input.glyphBox.x, input.glyphBox.z, input.bands),
                     float2(xs.z, y), m.y, true); v.z = r.x; vw.z = r.y;
        r = scanBand(vBase + bandIndex(xs.w, input.glyphBox.x, input.glyphBox.z, input.bands),
                     float2(xs.w, y), m.y, true); v.w = r.x; vw.w = r.y;
    }

    float4 floorCov = min(abs(h), abs(v));
    return max(abs(lerp(v, h, step(vw, hw))), floorCov);
}

float jitter(uint i, uint ss, float m) {
    if (ss == 1u || m == 0.0) {
        return 0.0;
    }
    return (((float)i + 0.5) / (float)ss - 0.5) / m;
}

float tapOffset(uint tap, int origin, uint oversample, float pixelsPerEm) {
    if (oversample == 0u || pixelsPerEm == 0.0) {
        return 0.0;
    }
    float centre = ((float)origin + (float)tap + 0.5) / (float)oversample;
    return (centre - 0.5) / pixelsPerEm;
}

PixelOutput main(PixelInput input) {
    float2 emCoord = emCoordOf(input);
    float2 m = localScaleOf(input, emCoord);
    uint2 os   = max(subpixel[0].oversample, uint2(1u, 1u));
    uint2 taps = min(max(subpixel[0].taps, uint2(1u, 1u)), uint2(8u, 8u));

    uint  ss = min(max(subpixel[0].supersample, 1u), 4u);
    float invSS = 1.0 / float(ss * ss);

    float2 sampleM = m * float2(os);

    float3 channels = float3(0.0, 0.0, 0.0);
    for (uint ty = 0u; ty < taps.y; ++ty) {
        float dy = -tapOffset(ty, subpixel[0].origin.y, os.y, m.y);
        for (uint sy = 0u; sy < ss; ++sy) {
            float y = emCoord.y + dy + jitter(sy, ss, sampleM.y);

            uint width = taps.x * ss;
            for (uint k = 0u; k < width; k += 4u) {
                float4 xs = float4(0.0, 0.0, 0.0, 0.0);
                float4 wr = float4(0.0, 0.0, 0.0, 0.0);
                float4 wg = float4(0.0, 0.0, 0.0, 0.0);
                float4 wb = float4(0.0, 0.0, 0.0, 0.0);
                [unroll]
                for (uint lane = 0u; lane < 4u; ++lane) {
                    uint at = min(k + lane, width - 1u);
                    uint tx = at;
                    uint sx = 0u;
                    if (ss != 1u) {
                        tx = at / ss;
                        sx = at - tx * ss;
                    }
                    float x = emCoord.x
                            + tapOffset(tx, subpixel[0].origin.x, os.x, m.x)
                            + jitter(sx, ss, sampleM.x);
                    float live = (k + lane < width) ? 1.0 : 0.0;
                    uint idx = ty * taps.x + tx;
                    xs[lane] = x;
                    wr[lane] = live * subpixel[0].weights[idx];
                    wg[lane] = live * subpixel[0].weights[64u + idx];
                    wb[lane] = live * subpixel[0].weights[128u + idx];
                }

                float4 cov = saturate(abs(windingRow4(input, xs, y, sampleM))) * invSS;
                channels.r += dot(wr, cov);
                channels.g += dot(wg, cov);
                channels.b += dot(wb, cov);
            }
        }
    }

    if (subpixel[0].channels < 2u) {
        channels = float3(channels.r, channels.r, channels.r);
    }
    channels = saturate(channels);

    PixelOutput output;
    output.color   = float4(input.glyphTint.rgb, 1.0);
    output.coverage = float4(input.glyphTint.a * channels,
                             input.glyphTint.a * max(channels.r, max(channels.g, channels.b)));
    return output;
}

#endif

#endif
