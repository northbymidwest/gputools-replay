//! `MTLPixelFormat` metadata: structural decomposition of colour, depth/stencil,
//! and block-compressed formats.
//!
//! Raw format values are verified against `objc2-metal`'s `MTLPixelFormat`
//! associated constants, not hand-derived from Apple's documentation.

use objc2_metal::{MTLPixelFormat, MTLTextureType, MTLTextureUsage};

/// The structural interpretation of an `MTLPixelFormat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatKind {
    /// Ordinary or packed colour.
    Color(ColorFormat),
    /// Depth and/or stencil.
    DepthStencil(DepthStencilFormat),
    /// Block-compressed (descriptive; `Texture::blocks` yields raw blocks).
    Compressed(CompressedFormat),
    /// A format the table does not describe. Raw bytes are still available.
    Unknown,
}

impl FormatKind {
    /// Bytes per pixel, for `Color`/`DepthStencil` (`None` for compressed/unknown).
    pub fn bytes_per_pixel(&self) -> Option<usize> {
        match self {
            FormatKind::Color(c) => Some(c.bytes_per_pixel),
            FormatKind::DepthStencil(d) => Some(d.bytes_per_pixel),
            FormatKind::Compressed(_) | FormatKind::Unknown => None,
        }
    }

    /// Whether this format's pixels are byte-aligned and castable as a flat `[P]`.
    pub fn byte_aligned(&self) -> bool {
        match self {
            FormatKind::Color(c) => c.byte_aligned,
            // A single aspect (depth-only or stencil-only) is a flat array of
            // that aspect's element type. Combined depth+stencil is not.
            FormatKind::DepthStencil(d) => d.depth.is_some() != d.stencil.is_some(),
            FormatKind::Compressed(_) | FormatKind::Unknown => false,
        }
    }

    /// The sRGB transfer flag.
    pub fn is_srgb(&self) -> bool {
        match self {
            FormatKind::Color(c) => c.srgb,
            FormatKind::Compressed(k) => k.srgb,
            FormatKind::DepthStencil(_) | FormatKind::Unknown => false,
        }
    }

    /// True for a combined depth+stencil format (both a depth and a stencil
    /// aspect present, e.g. `Depth32Float_Stencil8`). The fetch serves such a
    /// resource's aspects as separate per-aspect textures (depth 252 / stencil
    /// 261), never the combined format, so a combined descriptor never
    /// exact-matches a fetched texture.
    pub fn is_combined_depth_stencil(&self) -> bool {
        matches!(self, FormatKind::DepthStencil(d) if d.depth.is_some() && d.stencil.is_some())
    }

    /// True for a depth-only format (a depth aspect, no stencil).
    pub fn is_depth_only(&self) -> bool {
        matches!(self, FormatKind::DepthStencil(d) if d.depth.is_some() && d.stencil.is_none())
    }

    /// True for a stencil-only format (a stencil aspect, no depth).
    pub fn is_stencil_only(&self) -> bool {
        matches!(self, FormatKind::DepthStencil(d) if d.stencil.is_some() && d.depth.is_none())
    }
}

/// Colour format: channels (memory order), numeric encoding, transfer, stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorFormat {
    /// Channels in memory order (ordinary and packed both fit).
    pub channels: &'static [Channel],
    /// Numeric encoding, uniform across channels.
    pub numeric: NumericKind,
    /// The sRGB transfer flag (`_sRGB` variants). Orthogonal to `numeric`.
    pub srgb: bool,
    /// Total bytes per pixel.
    pub bytes_per_pixel: usize,
    /// All channels equal width and a multiple of 8 (clean typed cast).
    pub byte_aligned: bool,
}

/// One channel: which component, how many bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    /// Which colour component this channel carries.
    pub component: Component,
    /// Width of the channel, in bits.
    pub bits: u8,
}

/// A colour component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    /// Red.
    R,
    /// Green.
    G,
    /// Blue.
    B,
    /// Alpha.
    A,
}

/// The numeric encoding of a colour channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericKind {
    /// Unsigned normalized (`[0, 1]`).
    Unorm,
    /// Signed normalized (`[-1, 1]`).
    Snorm,
    /// Floating point.
    Float,
    /// Unsigned integer.
    Uint,
    /// Signed integer.
    Sint,
    /// Shared-exponent floating point (e.g. `RGB9E5Float`).
    SharedExponent,
}

/// Depth and/or stencil format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthStencilFormat {
    /// The depth aspect, if present.
    pub depth: Option<DepthKind>,
    /// The stencil aspect, if present.
    pub stencil: Option<StencilKind>,
    /// Bytes per pixel (includes any X-padding).
    pub bytes_per_pixel: usize,
}

/// Depth encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthKind {
    /// 16-bit unsigned normalized depth.
    Unorm16,
    /// 32-bit floating point depth.
    Float32,
}

/// Stencil encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StencilKind {
    /// 8-bit unsigned integer stencil.
    Uint8,
}

/// Block-compressed format geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedFormat {
    /// The compression scheme.
    pub scheme: CompressionScheme,
    /// Texels per block (e.g. `(4, 4)`).
    pub block: (u8, u8),
    /// Bytes per block.
    pub block_bytes: u8,
    /// The sRGB transfer flag.
    pub srgb: bool,
}

/// A block-compression scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionScheme {
    /// Adaptive Scalable Texture Compression.
    Astc,
    /// Ericsson Texture Compression 2.
    Etc2,
    /// Ericsson Alpha Compression (paired with ETC2 for alpha/single channels).
    Eac,
    /// PowerVR Texture Compression.
    Pvrtc,
    /// Block Compression (DirectX-style BCn).
    Bc,
    /// A scheme not otherwise distinguished by this table.
    Other,
}

/// Map a raw `MTLPixelFormat` value to the canonical enum.
pub fn mtl_format(raw: u32) -> MTLPixelFormat {
    MTLPixelFormat(raw as _)
}

/// The canonical Metal name for a known `MTLPixelFormat`, `None` for a format
/// the table does not describe. Mirrors [`format_kind`]'s arms.
#[allow(clippy::too_many_lines)]
pub fn name(fmt: MTLPixelFormat) -> Option<&'static str> {
    match fmt.0 as u32 {
        // -- Ordinary 1-channel colour --
        1 => Some("A8Unorm"),
        10 => Some("R8Unorm"),
        11 => Some("R8Unorm_sRGB"),
        12 => Some("R8Snorm"),
        13 => Some("R8Uint"),
        14 => Some("R8Sint"),
        20 => Some("R16Unorm"),
        22 => Some("R16Snorm"),
        23 => Some("R16Uint"),
        24 => Some("R16Sint"),
        25 => Some("R16Float"),
        53 => Some("R32Uint"),
        54 => Some("R32Sint"),
        55 => Some("R32Float"),

        // -- Ordinary 2-channel colour --
        30 => Some("RG8Unorm"),
        31 => Some("RG8Unorm_sRGB"),
        32 => Some("RG8Snorm"),
        33 => Some("RG8Uint"),
        34 => Some("RG8Sint"),
        60 => Some("RG16Unorm"),
        62 => Some("RG16Snorm"),
        63 => Some("RG16Uint"),
        64 => Some("RG16Sint"),
        65 => Some("RG16Float"),
        103 => Some("RG32Uint"),
        104 => Some("RG32Sint"),
        105 => Some("RG32Float"),

        // -- Packed 16-bit colour --
        40 => Some("B5G6R5Unorm"),
        41 => Some("A1BGR5Unorm"),
        42 => Some("ABGR4Unorm"),
        43 => Some("BGR5A1Unorm"),

        // -- Ordinary 4-channel 8-bit colour --
        70 => Some("RGBA8Unorm"),
        71 => Some("RGBA8Unorm_sRGB"),
        72 => Some("RGBA8Snorm"),
        73 => Some("RGBA8Uint"),
        74 => Some("RGBA8Sint"),
        80 => Some("BGRA8Unorm"),
        81 => Some("BGRA8Unorm_sRGB"),

        // -- Packed 32-bit colour --
        90 => Some("RGB10A2Unorm"),
        91 => Some("RGB10A2Uint"),
        92 => Some("RG11B10Float"),
        93 => Some("RGB9E5Float"),
        94 => Some("BGR10A2Unorm"),

        // -- Ordinary 4-channel 16-bit colour --
        110 => Some("RGBA16Unorm"),
        112 => Some("RGBA16Snorm"),
        113 => Some("RGBA16Uint"),
        114 => Some("RGBA16Sint"),
        115 => Some("RGBA16Float"),

        // -- Ordinary 4-channel 32-bit colour --
        123 => Some("RGBA32Uint"),
        124 => Some("RGBA32Sint"),
        125 => Some("RGBA32Float"),

        // -- BC (DirectX-style block compression) --
        130 => Some("BC1_RGBA"),
        131 => Some("BC1_RGBA_sRGB"),
        132 => Some("BC2_RGBA"),
        133 => Some("BC2_RGBA_sRGB"),
        134 => Some("BC3_RGBA"),
        135 => Some("BC3_RGBA_sRGB"),
        140 => Some("BC4_RUnorm"),
        141 => Some("BC4_RSnorm"),
        142 => Some("BC5_RGUnorm"),
        143 => Some("BC5_RGSnorm"),
        150 => Some("BC6H_RGBFloat"),
        151 => Some("BC6H_RGBUfloat"),
        152 => Some("BC7_RGBAUnorm"),
        153 => Some("BC7_RGBAUnorm_sRGB"),

        // -- PVRTC --
        160 => Some("PVRTC_RGB_2BPP"),
        161 => Some("PVRTC_RGB_2BPP_sRGB"),
        162 => Some("PVRTC_RGB_4BPP"),
        163 => Some("PVRTC_RGB_4BPP_sRGB"),
        164 => Some("PVRTC_RGBA_2BPP"),
        165 => Some("PVRTC_RGBA_2BPP_sRGB"),
        166 => Some("PVRTC_RGBA_4BPP"),
        167 => Some("PVRTC_RGBA_4BPP_sRGB"),

        // -- EAC / ETC2 --
        170 => Some("EAC_R11Unorm"),
        172 => Some("EAC_R11Snorm"),
        174 => Some("EAC_RG11Unorm"),
        176 => Some("EAC_RG11Snorm"),
        178 => Some("EAC_RGBA8"),
        179 => Some("EAC_RGBA8_sRGB"),
        180 => Some("ETC2_RGB8"),
        181 => Some("ETC2_RGB8_sRGB"),
        182 => Some("ETC2_RGB8A1"),
        183 => Some("ETC2_RGB8A1_sRGB"),

        // -- ASTC (sRGB variants) --
        186 => Some("ASTC_4x4_sRGB"),
        187 => Some("ASTC_5x4_sRGB"),
        188 => Some("ASTC_5x5_sRGB"),
        189 => Some("ASTC_6x5_sRGB"),
        190 => Some("ASTC_6x6_sRGB"),
        192 => Some("ASTC_8x5_sRGB"),
        193 => Some("ASTC_8x6_sRGB"),
        194 => Some("ASTC_8x8_sRGB"),
        195 => Some("ASTC_10x5_sRGB"),
        196 => Some("ASTC_10x6_sRGB"),
        197 => Some("ASTC_10x8_sRGB"),
        198 => Some("ASTC_10x10_sRGB"),
        199 => Some("ASTC_12x10_sRGB"),
        200 => Some("ASTC_12x12_sRGB"),

        // -- ASTC (LDR) --
        204 => Some("ASTC_4x4_LDR"),
        205 => Some("ASTC_5x4_LDR"),
        206 => Some("ASTC_5x5_LDR"),
        207 => Some("ASTC_6x5_LDR"),
        208 => Some("ASTC_6x6_LDR"),
        210 => Some("ASTC_8x5_LDR"),
        211 => Some("ASTC_8x6_LDR"),
        212 => Some("ASTC_8x8_LDR"),
        213 => Some("ASTC_10x5_LDR"),
        214 => Some("ASTC_10x6_LDR"),
        215 => Some("ASTC_10x8_LDR"),
        216 => Some("ASTC_10x10_LDR"),
        217 => Some("ASTC_12x10_LDR"),
        218 => Some("ASTC_12x12_LDR"),

        // -- ASTC (HDR) --
        222 => Some("ASTC_4x4_HDR"),
        223 => Some("ASTC_5x4_HDR"),
        224 => Some("ASTC_5x5_HDR"),
        225 => Some("ASTC_6x5_HDR"),
        226 => Some("ASTC_6x6_HDR"),
        228 => Some("ASTC_8x5_HDR"),
        229 => Some("ASTC_8x6_HDR"),
        230 => Some("ASTC_8x8_HDR"),
        231 => Some("ASTC_10x5_HDR"),
        232 => Some("ASTC_10x6_HDR"),
        233 => Some("ASTC_10x8_HDR"),
        234 => Some("ASTC_10x10_HDR"),
        235 => Some("ASTC_12x10_HDR"),
        236 => Some("ASTC_12x12_HDR"),

        // -- Depth / stencil --
        250 => Some("Depth16Unorm"),
        252 => Some("Depth32Float"),
        253 => Some("Stencil8"),
        260 => Some("Depth32Float_Stencil8"),
        261 => Some("X32_Stencil8"),
        262 => Some("X24_Stencil8"),

        _ => None,
    }
}

/// The canonical Metal name for a known `MTLTextureType`, `None` for a raw
/// value the table does not describe. Raw values verified against
/// `objc2-metal 0.3.2`'s `MTLTextureType` associated constants.
pub fn texture_type_name(t: MTLTextureType) -> Option<&'static str> {
    match t.0 as u64 {
        0 => Some("Type1D"),
        1 => Some("Type1DArray"),
        2 => Some("Type2D"),
        3 => Some("Type2DArray"),
        4 => Some("Type2DMultisample"),
        5 => Some("TypeCube"),
        6 => Some("TypeCubeArray"),
        7 => Some("Type3D"),
        8 => Some("Type2DMultisampleArray"),
        9 => Some("TypeTextureBuffer"),
        _ => None,
    }
}

/// The names of `u`'s set bits, in a stable order (`ShaderRead`,
/// `ShaderWrite`, `RenderTarget`, `PixelFormatView`, `ShaderAtomic`). Empty
/// for `MTLTextureUsage::Unknown` (0) or any combination of unrecognised
/// bits. Bit values verified against `objc2-metal 0.3.2`'s `MTLTextureUsage`
/// associated constants.
pub fn usage_flag_names(u: MTLTextureUsage) -> Vec<&'static str> {
    const FLAGS: &[(u64, &str)] = &[
        (0x0001, "ShaderRead"),
        (0x0002, "ShaderWrite"),
        (0x0004, "RenderTarget"),
        (0x0010, "PixelFormatView"),
        (0x0020, "ShaderAtomic"),
    ];
    let raw = u.0 as u64;
    FLAGS
        .iter()
        .filter(|&&(bit, _)| raw & bit != 0)
        .map(|&(_, name)| name)
        .collect()
}

use Component::*;

const fn ch(component: Component, bits: u8) -> Channel {
    Channel { component, bits }
}

const A8: &[Channel] = &[ch(A, 8)];
const R8: &[Channel] = &[ch(R, 8)];
const RG8: &[Channel] = &[ch(R, 8), ch(G, 8)];
const RGBA8: &[Channel] = &[ch(R, 8), ch(G, 8), ch(B, 8), ch(A, 8)];
const BGRA8: &[Channel] = &[ch(B, 8), ch(G, 8), ch(R, 8), ch(A, 8)];
const R16: &[Channel] = &[ch(R, 16)];
const RG16: &[Channel] = &[ch(R, 16), ch(G, 16)];
const RGBA16: &[Channel] = &[ch(R, 16), ch(G, 16), ch(B, 16), ch(A, 16)];
const R32: &[Channel] = &[ch(R, 32)];
const RG32: &[Channel] = &[ch(R, 32), ch(G, 32)];
const RGBA32: &[Channel] = &[ch(R, 32), ch(G, 32), ch(B, 32), ch(A, 32)];
const B5G6R5: &[Channel] = &[ch(B, 5), ch(G, 6), ch(R, 5)];
const A1BGR5: &[Channel] = &[ch(A, 1), ch(B, 5), ch(G, 5), ch(R, 5)];
const ABGR4: &[Channel] = &[ch(A, 4), ch(B, 4), ch(G, 4), ch(R, 4)];
const BGR5A1: &[Channel] = &[ch(B, 5), ch(G, 5), ch(R, 5), ch(A, 1)];
const RGB10A2: &[Channel] = &[ch(R, 10), ch(G, 10), ch(B, 10), ch(A, 2)];
const BGR10A2: &[Channel] = &[ch(B, 10), ch(G, 10), ch(R, 10), ch(A, 2)];
const RG11B10: &[Channel] = &[ch(R, 11), ch(G, 11), ch(B, 10)];
const RGB9E5: &[Channel] = &[ch(R, 9), ch(G, 9), ch(B, 9)];

/// Build a `ColorFormat` from ordinary channels: `bytes_per_pixel` and
/// `byte_aligned` are derived from the channel widths. Not valid for formats
/// with bits outside the listed channels (e.g. `RGB9E5Float`'s shared
/// exponent) - construct those directly.
fn color(channels: &'static [Channel], numeric: NumericKind, srgb: bool) -> ColorFormat {
    let total_bits: u32 = channels.iter().map(|c| c.bits as u32).sum();
    let byte_aligned = channels
        .iter()
        .all(|c| c.bits == channels[0].bits && c.bits % 8 == 0);
    ColorFormat {
        channels,
        numeric,
        srgb,
        bytes_per_pixel: (total_bits / 8) as usize,
        byte_aligned,
    }
}

fn depth_stencil(
    depth: Option<DepthKind>,
    stencil: Option<StencilKind>,
    bytes_per_pixel: usize,
) -> DepthStencilFormat {
    DepthStencilFormat {
        depth,
        stencil,
        bytes_per_pixel,
    }
}

fn compressed(
    scheme: CompressionScheme,
    block: (u8, u8),
    block_bytes: u8,
    srgb: bool,
) -> CompressedFormat {
    CompressedFormat {
        scheme,
        block,
        block_bytes,
        srgb,
    }
}

/// Decompose a raw `MTLPixelFormat` value into its structural metadata.
///
/// Anything the table does not describe maps to [`FormatKind::Unknown`]; the
/// raw bytes remain available regardless.
#[allow(clippy::too_many_lines)]
pub fn format_kind(raw: u32) -> FormatKind {
    use CompressionScheme::*;
    use DepthKind::*;
    use FormatKind::{Color, Compressed, DepthStencil};
    use NumericKind::*;
    use StencilKind::Uint8;

    match raw {
        // -- Ordinary 1-channel colour --
        1 => Color(color(A8, Unorm, false)),   // A8Unorm
        10 => Color(color(R8, Unorm, false)),  // R8Unorm
        11 => Color(color(R8, Unorm, true)),   // R8Unorm_sRGB
        12 => Color(color(R8, Snorm, false)),  // R8Snorm
        13 => Color(color(R8, Uint, false)),   // R8Uint
        14 => Color(color(R8, Sint, false)),   // R8Sint
        20 => Color(color(R16, Unorm, false)), // R16Unorm
        22 => Color(color(R16, Snorm, false)), // R16Snorm
        23 => Color(color(R16, Uint, false)),  // R16Uint
        24 => Color(color(R16, Sint, false)),  // R16Sint
        25 => Color(color(R16, Float, false)), // R16Float
        53 => Color(color(R32, Uint, false)),  // R32Uint
        54 => Color(color(R32, Sint, false)),  // R32Sint
        55 => Color(color(R32, Float, false)), // R32Float

        // -- Ordinary 2-channel colour --
        30 => Color(color(RG8, Unorm, false)),   // RG8Unorm
        31 => Color(color(RG8, Unorm, true)),    // RG8Unorm_sRGB
        32 => Color(color(RG8, Snorm, false)),   // RG8Snorm
        33 => Color(color(RG8, Uint, false)),    // RG8Uint
        34 => Color(color(RG8, Sint, false)),    // RG8Sint
        60 => Color(color(RG16, Unorm, false)),  // RG16Unorm
        62 => Color(color(RG16, Snorm, false)),  // RG16Snorm
        63 => Color(color(RG16, Uint, false)),   // RG16Uint
        64 => Color(color(RG16, Sint, false)),   // RG16Sint
        65 => Color(color(RG16, Float, false)),  // RG16Float
        103 => Color(color(RG32, Uint, false)),  // RG32Uint
        104 => Color(color(RG32, Sint, false)),  // RG32Sint
        105 => Color(color(RG32, Float, false)), // RG32Float

        // -- Packed 16-bit colour --
        40 => Color(color(B5G6R5, Unorm, false)), // B5G6R5Unorm
        41 => Color(color(A1BGR5, Unorm, false)), // A1BGR5Unorm
        42 => Color(color(ABGR4, Unorm, false)),  // ABGR4Unorm
        43 => Color(color(BGR5A1, Unorm, false)), // BGR5A1Unorm

        // -- Ordinary 4-channel 8-bit colour --
        70 => Color(color(RGBA8, Unorm, false)), // RGBA8Unorm
        71 => Color(color(RGBA8, Unorm, true)),  // RGBA8Unorm_sRGB
        72 => Color(color(RGBA8, Snorm, false)), // RGBA8Snorm
        73 => Color(color(RGBA8, Uint, false)),  // RGBA8Uint
        74 => Color(color(RGBA8, Sint, false)),  // RGBA8Sint
        80 => Color(color(BGRA8, Unorm, false)), // BGRA8Unorm
        81 => Color(color(BGRA8, Unorm, true)),  // BGRA8Unorm_sRGB

        // -- Packed 32-bit colour --
        90 => Color(color(RGB10A2, Unorm, false)), // RGB10A2Unorm
        91 => Color(color(RGB10A2, Uint, false)),  // RGB10A2Uint
        92 => Color(color(RG11B10, Float, false)), // RG11B10Float
        93 => Color(ColorFormat {
            // RGB9E5Float: 9+9+9 mantissa bits plus a 5-bit shared exponent
            // that belongs to no single channel, so `bytes_per_pixel` (4) is
            // not derivable from the channel widths alone (27 bits).
            channels: RGB9E5,
            numeric: SharedExponent,
            srgb: false,
            bytes_per_pixel: 4,
            byte_aligned: false,
        }),
        94 => Color(color(BGR10A2, Unorm, false)), // BGR10A2Unorm

        // -- Ordinary 4-channel 16-bit colour --
        110 => Color(color(RGBA16, Unorm, false)), // RGBA16Unorm
        112 => Color(color(RGBA16, Snorm, false)), // RGBA16Snorm
        113 => Color(color(RGBA16, Uint, false)),  // RGBA16Uint
        114 => Color(color(RGBA16, Sint, false)),  // RGBA16Sint
        115 => Color(color(RGBA16, Float, false)), // RGBA16Float

        // -- Ordinary 4-channel 32-bit colour --
        123 => Color(color(RGBA32, Uint, false)), // RGBA32Uint
        124 => Color(color(RGBA32, Sint, false)), // RGBA32Sint
        125 => Color(color(RGBA32, Float, false)), // RGBA32Float

        // -- BC (DirectX-style block compression) --
        130 => Compressed(compressed(Bc, (4, 4), 8, false)), // BC1_RGBA
        131 => Compressed(compressed(Bc, (4, 4), 8, true)),  // BC1_RGBA_sRGB
        132 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC2_RGBA
        133 => Compressed(compressed(Bc, (4, 4), 16, true)), // BC2_RGBA_sRGB
        134 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC3_RGBA
        135 => Compressed(compressed(Bc, (4, 4), 16, true)), // BC3_RGBA_sRGB
        140 => Compressed(compressed(Bc, (4, 4), 8, false)), // BC4_RUnorm
        141 => Compressed(compressed(Bc, (4, 4), 8, false)), // BC4_RSnorm
        142 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC5_RGUnorm
        143 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC5_RGSnorm
        150 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC6H_RGBFloat
        151 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC6H_RGBUfloat
        152 => Compressed(compressed(Bc, (4, 4), 16, false)), // BC7_RGBAUnorm
        153 => Compressed(compressed(Bc, (4, 4), 16, true)), // BC7_RGBAUnorm_sRGB

        // -- PVRTC --
        160 => Compressed(compressed(Pvrtc, (8, 4), 8, false)), // PVRTC_RGB_2BPP
        161 => Compressed(compressed(Pvrtc, (8, 4), 8, true)),  // PVRTC_RGB_2BPP_sRGB
        162 => Compressed(compressed(Pvrtc, (4, 4), 8, false)), // PVRTC_RGB_4BPP
        163 => Compressed(compressed(Pvrtc, (4, 4), 8, true)),  // PVRTC_RGB_4BPP_sRGB
        164 => Compressed(compressed(Pvrtc, (8, 4), 8, false)), // PVRTC_RGBA_2BPP
        165 => Compressed(compressed(Pvrtc, (8, 4), 8, true)),  // PVRTC_RGBA_2BPP_sRGB
        166 => Compressed(compressed(Pvrtc, (4, 4), 8, false)), // PVRTC_RGBA_4BPP
        167 => Compressed(compressed(Pvrtc, (4, 4), 8, true)),  // PVRTC_RGBA_4BPP_sRGB

        // -- EAC / ETC2 --
        170 => Compressed(compressed(Eac, (4, 4), 8, false)), // EAC_R11Unorm
        172 => Compressed(compressed(Eac, (4, 4), 8, false)), // EAC_R11Snorm
        174 => Compressed(compressed(Eac, (4, 4), 16, false)), // EAC_RG11Unorm
        176 => Compressed(compressed(Eac, (4, 4), 16, false)), // EAC_RG11Snorm
        178 => Compressed(compressed(Eac, (4, 4), 16, false)), // EAC_RGBA8
        179 => Compressed(compressed(Eac, (4, 4), 16, true)), // EAC_RGBA8_sRGB
        180 => Compressed(compressed(Etc2, (4, 4), 8, false)), // ETC2_RGB8
        181 => Compressed(compressed(Etc2, (4, 4), 8, true)), // ETC2_RGB8_sRGB
        182 => Compressed(compressed(Etc2, (4, 4), 8, false)), // ETC2_RGB8A1
        183 => Compressed(compressed(Etc2, (4, 4), 8, true)), // ETC2_RGB8A1_sRGB

        // -- ASTC (sRGB variants) --
        186 => Compressed(compressed(Astc, (4, 4), 16, true)), // ASTC_4x4_sRGB
        187 => Compressed(compressed(Astc, (5, 4), 16, true)), // ASTC_5x4_sRGB
        188 => Compressed(compressed(Astc, (5, 5), 16, true)), // ASTC_5x5_sRGB
        189 => Compressed(compressed(Astc, (6, 5), 16, true)), // ASTC_6x5_sRGB
        190 => Compressed(compressed(Astc, (6, 6), 16, true)), // ASTC_6x6_sRGB
        192 => Compressed(compressed(Astc, (8, 5), 16, true)), // ASTC_8x5_sRGB
        193 => Compressed(compressed(Astc, (8, 6), 16, true)), // ASTC_8x6_sRGB
        194 => Compressed(compressed(Astc, (8, 8), 16, true)), // ASTC_8x8_sRGB
        195 => Compressed(compressed(Astc, (10, 5), 16, true)), // ASTC_10x5_sRGB
        196 => Compressed(compressed(Astc, (10, 6), 16, true)), // ASTC_10x6_sRGB
        197 => Compressed(compressed(Astc, (10, 8), 16, true)), // ASTC_10x8_sRGB
        198 => Compressed(compressed(Astc, (10, 10), 16, true)), // ASTC_10x10_sRGB
        199 => Compressed(compressed(Astc, (12, 10), 16, true)), // ASTC_12x10_sRGB
        200 => Compressed(compressed(Astc, (12, 12), 16, true)), // ASTC_12x12_sRGB

        // -- ASTC (LDR) --
        204 => Compressed(compressed(Astc, (4, 4), 16, false)), // ASTC_4x4_LDR
        205 => Compressed(compressed(Astc, (5, 4), 16, false)), // ASTC_5x4_LDR
        206 => Compressed(compressed(Astc, (5, 5), 16, false)), // ASTC_5x5_LDR
        207 => Compressed(compressed(Astc, (6, 5), 16, false)), // ASTC_6x5_LDR
        208 => Compressed(compressed(Astc, (6, 6), 16, false)), // ASTC_6x6_LDR
        210 => Compressed(compressed(Astc, (8, 5), 16, false)), // ASTC_8x5_LDR
        211 => Compressed(compressed(Astc, (8, 6), 16, false)), // ASTC_8x6_LDR
        212 => Compressed(compressed(Astc, (8, 8), 16, false)), // ASTC_8x8_LDR
        213 => Compressed(compressed(Astc, (10, 5), 16, false)), // ASTC_10x5_LDR
        214 => Compressed(compressed(Astc, (10, 6), 16, false)), // ASTC_10x6_LDR
        215 => Compressed(compressed(Astc, (10, 8), 16, false)), // ASTC_10x8_LDR
        216 => Compressed(compressed(Astc, (10, 10), 16, false)), // ASTC_10x10_LDR
        217 => Compressed(compressed(Astc, (12, 10), 16, false)), // ASTC_12x10_LDR
        218 => Compressed(compressed(Astc, (12, 12), 16, false)), // ASTC_12x12_LDR

        // -- ASTC (HDR) --
        222 => Compressed(compressed(Astc, (4, 4), 16, false)), // ASTC_4x4_HDR
        223 => Compressed(compressed(Astc, (5, 4), 16, false)), // ASTC_5x4_HDR
        224 => Compressed(compressed(Astc, (5, 5), 16, false)), // ASTC_5x5_HDR
        225 => Compressed(compressed(Astc, (6, 5), 16, false)), // ASTC_6x5_HDR
        226 => Compressed(compressed(Astc, (6, 6), 16, false)), // ASTC_6x6_HDR
        228 => Compressed(compressed(Astc, (8, 5), 16, false)), // ASTC_8x5_HDR
        229 => Compressed(compressed(Astc, (8, 6), 16, false)), // ASTC_8x6_HDR
        230 => Compressed(compressed(Astc, (8, 8), 16, false)), // ASTC_8x8_HDR
        231 => Compressed(compressed(Astc, (10, 5), 16, false)), // ASTC_10x5_HDR
        232 => Compressed(compressed(Astc, (10, 6), 16, false)), // ASTC_10x6_HDR
        233 => Compressed(compressed(Astc, (10, 8), 16, false)), // ASTC_10x8_HDR
        234 => Compressed(compressed(Astc, (10, 10), 16, false)), // ASTC_10x10_HDR
        235 => Compressed(compressed(Astc, (12, 10), 16, false)), // ASTC_12x10_HDR
        236 => Compressed(compressed(Astc, (12, 12), 16, false)), // ASTC_12x12_HDR

        // -- Depth / stencil --
        250 => DepthStencil(depth_stencil(Some(Unorm16), None, 2)), // Depth16Unorm
        252 => DepthStencil(depth_stencil(Some(Float32), None, 4)), // Depth32Float
        253 => DepthStencil(depth_stencil(None, Some(Uint8), 1)),   // Stencil8
        260 => DepthStencil(depth_stencil(Some(Float32), Some(Uint8), 8)), // Depth32Float_Stencil8
        // NOTE: the task brief's raw values for X24_Stencil8/X32_Stencil8
        // (261/262) are swapped relative to objc2-metal's own constants -
        // verified against MTLPixelFormat::X32_Stencil8 = 261 and
        // MTLPixelFormat::X24_Stencil8 = 262. Using objc2-metal's values.
        261 => DepthStencil(depth_stencil(None, Some(Uint8), 8)), // X32_Stencil8
        262 => DepthStencil(depth_stencil(None, Some(Uint8), 4)), // X24_Stencil8

        _ => FormatKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra8_is_byte_aligned_unorm() {
        let FormatKind::Color(c) = format_kind(80) else {
            panic!("BGRA8Unorm is color")
        };
        assert_eq!(c.bytes_per_pixel, 4);
        assert!(c.byte_aligned && !c.srgb);
        assert_eq!(c.numeric, NumericKind::Unorm);
        assert_eq!(
            c.channels.iter().map(|ch| ch.component).collect::<Vec<_>>(),
            vec![Component::B, Component::G, Component::R, Component::A]
        );
    }

    #[test]
    fn rgba32float_is_float() {
        let FormatKind::Color(c) = format_kind(125) else {
            panic!()
        }; // RGBA32Float
        assert_eq!(c.numeric, NumericKind::Float);
        assert_eq!(c.bytes_per_pixel, 16);
    }

    #[test]
    fn depth32float_is_depth() {
        let FormatKind::DepthStencil(d) = format_kind(252) else {
            panic!()
        };
        assert_eq!(d.depth, Some(DepthKind::Float32));
        assert_eq!(d.stencil, None);
    }

    #[test]
    fn astc_4x4_is_compressed() {
        let FormatKind::Compressed(k) = format_kind(204) else {
            panic!()
        };
        assert_eq!(k.block, (4, 4));
        assert_eq!(k.block_bytes, 16);
    }

    #[test]
    fn unknown_is_unknown() {
        assert!(matches!(format_kind(0xffff_ff00), FormatKind::Unknown));
    }

    #[test]
    fn rgb10a2_is_packed_not_byte_aligned() {
        let FormatKind::Color(c) = format_kind(90) else {
            panic!("RGB10A2Unorm is color")
        }; // RGB10A2Unorm
        assert_eq!(
            c.channels
                .iter()
                .map(|ch| (ch.component, ch.bits))
                .collect::<Vec<_>>(),
            vec![
                (Component::R, 10),
                (Component::G, 10),
                (Component::B, 10),
                (Component::A, 2)
            ]
        );
        assert!(!c.byte_aligned);
        assert_eq!(c.bytes_per_pixel, 4);
    }

    #[test]
    fn rgba8_srgb_is_srgb() {
        let kind = format_kind(71); // RGBA8Unorm_sRGB
        let FormatKind::Color(c) = &kind else {
            panic!("RGBA8Unorm_sRGB is color")
        };
        assert!(c.srgb);
        assert!(kind.is_srgb());
    }

    #[test]
    fn depth_only_and_stencil_only() {
        // Depth32Float: depth only.
        assert!(format_kind(252).is_depth_only());
        assert!(!format_kind(252).is_stencil_only());

        // Stencil8, X32_Stencil8, X24_Stencil8: stencil only.
        for raw in [253, 261, 262] {
            assert!(format_kind(raw).is_stencil_only(), "raw {raw}");
            assert!(!format_kind(raw).is_depth_only(), "raw {raw}");
        }

        // Depth32Float_Stencil8: combined, neither depth-only nor stencil-only.
        assert!(!format_kind(260).is_depth_only());
        assert!(!format_kind(260).is_stencil_only());

        // BGRA8Unorm: not depth/stencil at all.
        assert!(!format_kind(80).is_depth_only());
        assert!(!format_kind(80).is_stencil_only());
    }

    #[test]
    fn name_returns_canonical_metal_names() {
        assert_eq!(name(mtl_format(80)), Some("BGRA8Unorm"));
        assert_eq!(name(mtl_format(252)), Some("Depth32Float"));
        assert_eq!(name(mtl_format(260)), Some("Depth32Float_Stencil8"));
        assert_eq!(name(mtl_format(204)), Some("ASTC_4x4_LDR"));
        assert_eq!(name(mtl_format(125)), Some("RGBA32Float"));
        assert_eq!(name(mtl_format(0xffff_ff00)), None);
    }

    #[test]
    fn texture_type_name_returns_canonical_names() {
        assert_eq!(texture_type_name(MTLTextureType::Type3D), Some("Type3D"));
        assert_eq!(texture_type_name(MTLTextureType::Type2D), Some("Type2D"));
        assert_eq!(
            texture_type_name(MTLTextureType::TypeCubeArray),
            Some("TypeCubeArray")
        );
        assert_eq!(texture_type_name(MTLTextureType(99)), None);
    }

    #[test]
    fn usage_flag_names_lists_set_bits_in_order() {
        let u = MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget;
        assert_eq!(usage_flag_names(u), vec!["ShaderRead", "RenderTarget"]);
        assert!(usage_flag_names(MTLTextureUsage::Unknown).is_empty());
    }
}
