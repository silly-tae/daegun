// `f32x4` is four floats in a struct, not a SIMD type. The walkers stay scalar on purpose:
// depositing is a scatter-add whose consecutive steps can collide, and LLVM finds no
// vectorization in them anyway – checked in the disassembly against a `[f32; 4]` rewrite.
pub mod platform;
pub mod simd;
pub mod math;

// `daegpu` is not a second implementation of this. It answers what a glyph *is*, leaving the
// size to the shader; this answers what it looks like at one exact size. Neither derives the other.
pub mod rasterize;
