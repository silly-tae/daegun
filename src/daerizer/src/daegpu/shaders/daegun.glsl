#if defined(DAEGUN_VERTEX) || defined(DAEGUN_SUBPIXEL)
layout(std430, binding = 4) readonly buffer SubpixelBuffer {
    float subpixelWeights[192];
    uvec2 subpixelOversample;
    uvec2 subpixelTaps;
    ivec2 subpixelOrigin;
    uint  subpixelChannels;
    uint  subpixelSupersample;
};
#endif

#if __VERSION__ >= 400
#define DAEGUN_PRECISE precise
#else
#define DAEGUN_PRECISE
#endif

#ifdef DAEGUN_VERTEX

struct GlyphInstance {
    vec4  glyphBox;
    vec4  tint;
    vec2  offset;
    vec2  emPixels;
    float scale;
    uint  bandBase;
    uint  bandsPerAxis;
    uint  hullBase;
    float invScale;
    // Three scalars and not a 3-vector: a three-component vector aligns to 16, which puts it at
    // offset 80 and makes the struct 96 while Rust says 80 – every instance after the first then
    // reads from the wrong stride.
    float _pad0;
    float _pad1;
    float _pad2;
};

layout(std430, binding = 3) readonly buffer InstanceBuffer {
    GlyphInstance instances[];
};

struct HullVertex {
    vec2 pos;
    vec2 dilateX;
    vec2 dilateY;
};

layout(std430, binding = 5) readonly buffer HullBuffer {
    HullVertex hull[];
};

struct DrawUniform {
    mat4 projection;
    vec4 view;
};

#ifdef DAEGUN_VULKAN
layout(std140, binding = 6) uniform ProjectionBuffer {
    DrawUniform uDraw;
};
#else
uniform DrawUniform uDraw;
#endif

#ifdef DAEGUN_VULKAN
#define DAEGUN_INSTANCE_ID gl_InstanceIndex
#define DAEGUN_VERTEX_ID   gl_VertexIndex
#else
#define DAEGUN_INSTANCE_ID gl_InstanceID
#define DAEGUN_VERTEX_ID   gl_VertexID
#endif

layout(location = 0) out vec2 emCoord;
layout(location = 6) flat out vec4 emAnchor;
layout(location = 7) flat out float emExact;
layout(location = 1) flat out uint bandBase;
layout(location = 2) flat out uint bandCount;
layout(location = 3) flat out vec4 glyphBox;
layout(location = 4) flat out vec4 glyphTint;
layout(location = 5) flat out vec2 emPixels;

vec2 dilationPixels() {
    vec2 d = vec2(0.5);
    if (subpixelOrigin.x < 0 && subpixelOversample.x > 0u) {
        d.x = max(0.5, ceil(float(-subpixelOrigin.x) / float(subpixelOversample.x)));
    }
    if (subpixelOrigin.y < 0 && subpixelOversample.y > 0u) {
        d.y = max(0.5, ceil(float(-subpixelOrigin.y) / float(subpixelOversample.y)));
    }
    return d;
}

void main() {
    GlyphInstance glyph = instances[DAEGUN_INSTANCE_ID];
    HullVertex corner = hull[glyph.hullBase + uint(DAEGUN_VERTEX_ID)];

    vec2 pad = dilationPixels() / max(glyph.emPixels, vec2(1.0e-6));

    bandBase  = glyph.bandBase;
    bandCount = glyph.bandsPerAxis;
    glyphBox  = glyph.glyphBox;
    glyphTint = glyph.tint;
    emPixels  = glyph.emPixels;

    emCoord = corner.pos + vec2(dot(corner.dilateX, pad), dot(corner.dilateY, pad));
    gl_Position = uDraw.projection * vec4(glyph.offset + emCoord * glyph.scale, 0.0, 1.0);
    emAnchor = vec4(glyph.offset.x, uDraw.view.x - glyph.offset.y, glyph.invScale, glyph.scale);
    emExact  = uDraw.view.y;
}

#endif

#if defined(DAEGUN_FRAGMENT) || defined(DAEGUN_SUBPIXEL)

layout(std430, binding = 0) readonly buffer CurveBuffer {
    vec2 curvePoints[];
};

layout(std430, binding = 1) readonly buffer BandCurveBuffer {
    uint bandCurves[];
};

layout(std430, binding = 2) readonly buffer BandBuffer {
    uvec2 bands[];
};

layout(location = 0) in vec2 emCoord;
layout(location = 6) flat in vec4 emAnchor;
layout(location = 7) flat in float emExact;
layout(location = 1) flat in uint bandBase;
layout(location = 2) flat in uint bandCount;
layout(location = 3) flat in vec4 glyphBox;
layout(location = 4) flat in vec4 glyphTint;
layout(location = 5) flat in vec2 emPixels;

#ifdef DAEGUN_FRAGMENT

layout(location = 0) out vec4 fragColor;

#endif

#ifdef DAEGUN_SUBPIXEL

layout(location = 0, index = 0) out vec4 fragColor;
layout(location = 0, index = 1) out vec4 fragCoverage;

#endif

vec2 localScale(vec2 emCoord);

vec2 localScaleOf(vec2 emCoord) {
    if (emExact != 0.0) {
        return vec2(emAnchor.w, emAnchor.w);
    }
    return localScale(emCoord);
}

vec2 emCoordOf() {
    if (emExact == 0.0) {
        return emCoord;
    }
    return vec2((gl_FragCoord.x - emAnchor.x) * emAnchor.z,
                (emAnchor.y - gl_FragCoord.y) * emAnchor.z);
}

vec2 localScale(vec2 emCoord) {
    vec2 dx = dFdx(emCoord);
    vec2 dy = dFdy(emCoord);
    float det = dx.x * dy.y - dy.x * dx.y;
    if (abs(det) < 1.0e-20) {
        return emPixels;
    }
    float inv = 1.0 / abs(det);
    return vec2(length(vec2(dy.y, -dx.y)), length(vec2(-dy.x, dx.x))) * inv;
}

uint rootContribution(float y1, float y2, float y3) {
    uint signCode = (y1 > 0.0 ? 2u : 0u) + (y2 > 0.0 ? 4u : 0u) + (y3 > 0.0 ? 8u : 0u);
    return (0x2E74u >> signCode) & 3u;
}

float curveX(vec2 p1, vec2 p2, vec2 p3, float t) {
    float u = 1.0 - t;
    return u * u * p1.x + 2.0 * t * u * p2.x + t * t * p3.x;
}

vec3 curveRoots(vec2 p1, vec2 p2, vec2 p3) {
    uint contribution = rootContribution(p1.y, p2.y, p3.y);
    if (contribution == 0u) {
        return vec3(0.0, 0.0, 0.0);
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
    return vec3(float(contribution), t1, t2);
}

vec2 curveCoverageAt(vec2 p1, vec2 p2, vec2 p3, vec3 roots, float pixelsPerEm) {
    uint contribution = uint(roots.x);
    float coverage = 0.0;
    float weight = 0.0;
    if ((contribution & 1u) != 0u) {
        float x = pixelsPerEm * curveX(p1, p2, p3, roots.y);
        coverage += clamp(x + 0.5, 0.0, 1.0);
        weight = max(weight, clamp(1.0 - 2.0 * abs(x), 0.0, 1.0));
    }
    if ((contribution & 2u) != 0u) {
        float x = pixelsPerEm * curveX(p1, p2, p3, roots.z);
        coverage -= clamp(x + 0.5, 0.0, 1.0);
        weight = max(weight, clamp(1.0 - 2.0 * abs(x), 0.0, 1.0));
    }
    return vec2(coverage, weight);
}

float combineRays(vec2 h, vec2 v) {
    float floorCov = min(abs(h.x), abs(v.x));
    return max(abs(h.y >= v.y ? h.x : v.x), floorCov);
}

vec2 curveCoverage(vec2 p1, vec2 p2, vec2 p3, float pixelsPerEm) {
    vec3 roots = curveRoots(p1, p2, p3);
    if (roots.x == 0.0) {
        return vec2(0.0);
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
    float f = (t - lo) / span * float(count);
    return uint(clamp(f, 0.0, float(count) - 1.0));
}

vec2 scanBand(uint band, vec2 origin, float pixelsPerEm, bool transpose) {
    uvec2 slice = bands[band];
    float coverage = 0.0;
    float weight = 0.0;

    for (uint i = 0u; i < slice.y; ++i) {
        uint base = bandCurves[slice.x + i] * 3u;

        vec2 p1 = curvePoints[base]     - origin;
        vec2 p2 = curvePoints[base + 1u] - origin;
        vec2 p3 = curvePoints[base + 2u] - origin;

        // A rotation, not an axis swap: swapping mirrors the plane, a mirror reverses every winding
        // sign, and the two rays would then cancel to zero everywhere instead of agreeing.
        if (transpose) {
            p1 = vec2(p1.y, -p1.x);
            p2 = vec2(p2.y, -p2.x);
            p3 = vec2(p3.y, -p3.x);
        }

        if (max(max(p1.x, p2.x), p3.x) * pixelsPerEm < -0.5) {
            break;
        }

        vec2 c = curveCoverage(p1, p2, p3, pixelsPerEm);
        coverage += c.x;
        weight = max(weight, c.y);
    }

    return vec2(coverage, weight);
}

float windingAt(vec2 at, vec2 m) {
    uint hBand = bandBase + bandIndex(at.y, glyphBox.y, glyphBox.w, bandCount);
    if (hBand < bandBase) { return 0.0; }
    uint vBand = bandBase + bandCount + bandIndex(at.x, glyphBox.x, glyphBox.z, bandCount);
    if (vBand < bandBase) { return 0.0; }

    return combineRays(scanBand(hBand, at, m.x, false), scanBand(vBand, at, m.y, true));
}

#ifdef DAEGUN_FRAGMENT

void main() {
    vec2 emCoord = emCoordOf();
    DAEGUN_PRECISE float coverage = windingAt(emCoord, localScaleOf(emCoord));

    fragColor = vec4(glyphTint.rgb, glyphTint.a * clamp(abs(coverage), 0.0, 1.0));
}

#endif

#ifdef DAEGUN_SUBPIXEL

vec4 scanBandRow4(uint band, vec4 xs, float y, float pixelsPerEm, out vec4 weightOut) {
    uvec2 slice = bands[band];
    vec4 coverage = vec4(0.0);
    vec4 weight = vec4(0.0);
    float minX = min(min(xs.x, xs.y), min(xs.z, xs.w));

    for (uint i = 0u; i < slice.y; ++i) {
        uint base = bandCurves[slice.x + i] * 3u;
        vec2 q1 = curvePoints[base];
        vec2 q2 = curvePoints[base + 1u];
        vec2 q3 = curvePoints[base + 2u];

        if ((max(max(q1.x, q2.x), q3.x) - minX) * pixelsPerEm < -0.5) {
            break;
        }

        float e1 = q1.y - y;
        float e2 = q2.y - y;
        float e3 = q3.y - y;
        vec3 roots = curveRoots(vec2(0.0, e1), vec2(0.0, e2), vec2(0.0, e3));
        if (roots.x == 0.0) {
            continue;
        }

        uint contribution = uint(roots.x);
        if ((contribution & 1u) != 0u) {
            vec4 x = pixelsPerEm * (curveX(q1, q2, q3, roots.y) - xs);
            coverage += clamp(x + 0.5, 0.0, 1.0);
            weight = max(weight, clamp(1.0 - 2.0 * abs(x), 0.0, 1.0));
        }
        if ((contribution & 2u) != 0u) {
            vec4 x = pixelsPerEm * (curveX(q1, q2, q3, roots.z) - xs);
            coverage -= clamp(x + 0.5, 0.0, 1.0);
            weight = max(weight, clamp(1.0 - 2.0 * abs(x), 0.0, 1.0));
        }
    }
    weightOut = weight;
    return coverage;
}

vec4 windingRow4(vec4 xs, float y, vec2 m) {
    uint hBand = bandBase + bandIndex(y, glyphBox.y, glyphBox.w, bandCount);
    if (hBand < bandBase) { return vec4(0.0); }

    vec4 hw;
    vec4 h = scanBandRow4(hBand, xs, y, m.x, hw);

    vec4 v = vec4(0.0);
    vec4 vw = vec4(0.0);
    uint vBase = bandBase + bandCount;
    if (vBase >= bandBase) {
        vec2 r;
        r = scanBand(vBase + bandIndex(xs.x, glyphBox.x, glyphBox.z, bandCount), vec2(xs.x, y), m.y, true);
        v.x = r.x; vw.x = r.y;
        r = scanBand(vBase + bandIndex(xs.y, glyphBox.x, glyphBox.z, bandCount), vec2(xs.y, y), m.y, true);
        v.y = r.x; vw.y = r.y;
        r = scanBand(vBase + bandIndex(xs.z, glyphBox.x, glyphBox.z, bandCount), vec2(xs.z, y), m.y, true);
        v.z = r.x; vw.z = r.y;
        r = scanBand(vBase + bandIndex(xs.w, glyphBox.x, glyphBox.z, bandCount), vec2(xs.w, y), m.y, true);
        v.w = r.x; vw.w = r.y;
    }

    vec4 floorCov = min(abs(h), abs(v));
    return max(abs(mix(v, h, greaterThanEqual(hw, vw))), floorCov);
}

float jitter(uint i, uint ss, float m) {
    if (ss == 1u || m == 0.0) {
        return 0.0;
    }
    return ((float(i) + 0.5) / float(ss) - 0.5) / m;
}

float tapOffset(uint tap, int origin, uint oversample, float pixelsPerEm) {
    if (oversample == 0u || pixelsPerEm == 0.0) {
        return 0.0;
    }
    float centre = (float(origin) + float(tap) + 0.5) / float(oversample);
    return (centre - 0.5) / pixelsPerEm;
}

void main() {
    vec2 emCoord = emCoordOf();
    vec2 m = localScaleOf(emCoord);
    uvec2 os   = max(subpixelOversample, uvec2(1u));
    uvec2 taps = min(max(subpixelTaps, uvec2(1u)), uvec2(8u));

    uint  ss = min(max(subpixelSupersample, 1u), 4u);
    float invSS = 1.0 / float(ss * ss);

    vec2 sampleM = m * vec2(os);

    vec3 channels = vec3(0.0);
    for (uint ty = 0u; ty < taps.y; ++ty) {
        float dy = -tapOffset(ty, subpixelOrigin.y, os.y, m.y);
        for (uint sy = 0u; sy < ss; ++sy) {
            float y = emCoord.y + dy + jitter(sy, ss, sampleM.y);

            uint width = taps.x * ss;
            for (uint k = 0u; k < width; k += 4u) {
                vec4 xs = vec4(0.0);
                vec4 wr = vec4(0.0);
                vec4 wg = vec4(0.0);
                vec4 wb = vec4(0.0);
                for (uint lane = 0u; lane < 4u; ++lane) {
                    uint at = min(k + lane, width - 1u);
                    uint tx = at;
                    uint sx = 0u;
                    if (ss != 1u) {
                        tx = at / ss;
                        sx = at - tx * ss;
                    }
                    float x = emCoord.x
                            + tapOffset(tx, subpixelOrigin.x, os.x, m.x)
                            + jitter(sx, ss, sampleM.x);
                    float live = (k + lane < width) ? 1.0 : 0.0;
                    uint idx = ty * taps.x + tx;
                    xs[lane] = x;
                    wr[lane] = live * subpixelWeights[idx];
                    wg[lane] = live * subpixelWeights[64u + idx];
                    wb[lane] = live * subpixelWeights[128u + idx];
                }

                vec4 cov = clamp(abs(windingRow4(xs, y, sampleM)), 0.0, 1.0) * invSS;
                channels.r += dot(wr, cov);
                channels.g += dot(wg, cov);
                channels.b += dot(wb, cov);
            }
        }
    }

    if (subpixelChannels < 2u) {
        channels = vec3(channels.r);
    }
    channels = clamp(channels, 0.0, 1.0);

    fragColor   = vec4(glyphTint.rgb, 1.0);
    fragCoverage = vec4(glyphTint.a * channels,
                        glyphTint.a * max(channels.r, max(channels.g, channels.b)));
}

#endif

#endif
