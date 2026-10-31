//! Decoding picture files into textures.
//!
//! * [`decode`] reads PNG, JPEG, BMP, TGA, GIF, WebP, TIFF and Radiance HDR files into 8-bit RGBA.
//! * [`with_mips`] builds the mip chain (averaged in linear light for colour pictures, so a far-away brick wall does not
//!   turn darker than the bricks), which is what keeps textures from shimmering when they are small on the screen.
//! * [`metallic_roughness`] combines separate roughness and metallic maps into the one picture the renderer reads.
//! * [`to_image`] makes a Bevy [`Image`] with the right colour space (colour maps are sRGB, data maps - normals, roughness -
//!   are linear) and the sampler (filter, wrapping, anisotropy) a material asks for.
//!
//! Nothing here touches the GPU, so all of it is tested without a window.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::Image,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::material::{AddressKind, FilterKind, SamplerSettings};

/// A picture larger than this in either direction is refused (a corrupt header must not make the engine allocate gigabytes).
pub const MAX_SIZE: u32 = 16_384;
/// ... and so is one with more pixels than this.
pub const MAX_PIXELS: u64 = 120_000_000;

/// A decoded picture: 8-bit RGBA, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Whether any pixel is not fully opaque.
    pub has_alpha: bool,
}

impl Decoded {
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Decoded {
        Decoded { width, height, rgba: rgba.iter().copied().cycle().take((width * height * 4) as usize).collect(), has_alpha: rgba[3] != 255 }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

/// Whether [`decode`] understands a file with this extension (dds and ktx2 are read by the renderer itself).
pub fn is_decodable(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp" | "tga" | "gif" | "webp" | "tif" | "tiff" | "hdr")
}

/// Reads a picture. `ext` (without the dot) is only a hint: the format is recognised from the bytes first.
pub fn decode(bytes: &[u8], ext: &str) -> Result<Decoded, String> {
    let guessed = image::guess_format(bytes).ok();
    let format = guessed.or_else(|| image::ImageFormat::from_extension(ext)).ok_or_else(|| format!("'.{ext}' is not a picture format Forge can read"))?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIZE);
    limits.max_image_height = Some(MAX_SIZE);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("the picture cannot be read: {e}"))?;
    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!("the picture is {width} x {height}, which is too large"));
    }
    let rgba = img.to_rgba8().into_raw();
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
    Ok(Decoded { width, height, rgba, has_alpha })
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

/// The number of levels of a full mip chain for a picture of this size.
pub fn mip_levels(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// One level down: every pixel is the average of the 2 x 2 above it (edges clamp, so odd sizes work). Colour channels of a
/// colour picture (`srgb`) are averaged in linear light, weighted by alpha so see-through pixels do not bleed; alpha and the
/// channels of a data picture are averaged as they are.
fn halve(src: &[u8], w: u32, h: u32, srgb: bool, table: &[f32; 256]) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let xs = [(x * 2).min(w - 1), (x * 2 + 1).min(w - 1)];
            let ys = [(y * 2).min(h - 1), (y * 2 + 1).min(h - 1)];
            let mut sum = [0f32; 3];
            let (mut alpha, mut weight) = (0f32, 0f32);
            for &sy in &ys {
                for &sx in &xs {
                    let i = ((sy * w + sx) * 4) as usize;
                    let a = f32::from(src[i + 3]) / 255.0;
                    // (A fully see-through pixel has no colour of its own to give.)
                    let wgt = if srgb { a.max(1e-4) } else { 1.0 };
                    for c in 0..3 {
                        let v = if srgb { table[src[i + c] as usize] } else { f32::from(src[i + c]) / 255.0 };
                        sum[c] += v * wgt;
                    }
                    alpha += a;
                    weight += wgt;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..3 {
                let avg = sum[c] / weight;
                out[o + c] = if srgb { linear_to_srgb(avg) } else { (avg * 255.0).round() as u8 };
            }
            out[o + 3] = (alpha / 4.0 * 255.0).round() as u8;
        }
    }
    (out, nw, nh)
}

/// The whole mip chain of a picture as one buffer (level 0 first), and the number of levels.
pub fn with_mips(d: &Decoded, srgb: bool) -> (Vec<u8>, u32) {
    let table: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as u8));
    let levels = mip_levels(d.width, d.height);
    let mut all = d.rgba.clone();
    let (mut cur, mut w, mut h) = (d.rgba.clone(), d.width, d.height);
    for _ in 1..levels {
        let (next, nw, nh) = halve(&cur, w, h, srgb, &table);
        all.extend_from_slice(&next);
        (cur, w, h) = (next, nw, nh);
    }
    (all, levels)
}

/// Resizes a picture to `width` x `height` (bilinear).
pub fn resized(d: &Decoded, width: u32, height: u32) -> Decoded {
    if (d.width, d.height) == (width, height) {
        return d.clone();
    }
    let Some(buffer) = image::RgbaImage::from_raw(d.width, d.height, d.rgba.clone()) else { return d.clone() };
    let out = image::imageops::resize(&buffer, width, height, image::imageops::FilterType::Triangle);
    Decoded { width, height, rgba: out.into_raw(), has_alpha: d.has_alpha }
}

/// The picture the renderer reads roughness and metallic from: green is roughness, blue is metallic (glTF's layout), each
/// taken from the red channel of its own map. A missing map leaves that channel at 255, which the material's own number then
/// scales (so a material with only a roughness map keeps its `metallic` value).
pub fn metallic_roughness(roughness: Option<&Decoded>, metallic: Option<&Decoded>) -> Option<Decoded> {
    let (w, h) = [roughness, metallic].into_iter().flatten().map(|d| (d.width, d.height)).max_by_key(|(w, h)| w * h)?;
    let r = roughness.map(|d| resized(d, w, h));
    let m = metallic.map(|d| resized(d, w, h));
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        if let Some(r) = &r {
            rgba[i * 4 + 1] = r.rgba[i * 4];
        }
        if let Some(m) = &m {
            rgba[i * 4 + 2] = m.rgba[i * 4];
        }
    }
    Some(Decoded { width: w, height: h, rgba, has_alpha: false })
}

/// The sampler a material's settings describe.
pub fn sampler(s: &SamplerSettings) -> ImageSampler {
    let address = match s.address {
        AddressKind::Repeat => ImageAddressMode::Repeat,
        AddressKind::Clamp => ImageAddressMode::ClampToEdge,
        AddressKind::Mirror => ImageAddressMode::MirrorRepeat,
    };
    let (filter, mip) = match s.filter {
        FilterKind::Linear => (ImageFilterMode::Linear, ImageFilterMode::Linear),
        FilterKind::Nearest => (ImageFilterMode::Nearest, ImageFilterMode::Nearest),
    };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: mip,
        anisotropy_clamp: if s.filter == FilterKind::Linear { u16::from(s.anisotropy.clamp(1, 16)) } else { 1 },
        ..ImageSamplerDescriptor::default()
    })
}

/// A Bevy image from a decoded picture: `srgb` pictures are colour (stored as sRGB, so the GPU converts when it reads), the
/// others are data (read as they are). With mip-maps unless the sampler is `Nearest`.
pub fn to_image(d: &Decoded, srgb: bool, settings: &SamplerSettings) -> Image {
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let (data, levels) = if settings.filter == FilterKind::Nearest { (d.rgba.clone(), 1) } else { with_mips(d, srgb) };
    let mut image = Image::new(Extent3d { width: d.width, height: d.height, depth_or_array_layers: 1 }, TextureDimension::D2, d.rgba.clone(), format, RenderAssetUsages::RENDER_WORLD);
    if levels > 1 {
        image.texture_descriptor.mip_level_count = levels;
        image.data = Some(data);
    }
    image.sampler = sampler(settings);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut buf = image::RgbaImage::new(w, h);
        for (x, y, p) in buf.enumerate_pixels_mut() {
            *p = image::Rgba(pixel(x, y));
        }
        let mut out = Vec::new();
        buf.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn pictures_are_decoded_whatever_the_extension_says() {
        let bytes = png(3, 2, |x, y| [x as u8 * 50, y as u8 * 100, 7, 255]);
        let d = decode(&bytes, "png").unwrap();
        assert_eq!((d.width, d.height, d.has_alpha), (3, 2, false));
        assert_eq!(d.pixel(2, 1), [100, 100, 7, 255]);
        assert_eq!(decode(&bytes, "jpg").unwrap().pixel(1, 0), [50, 0, 7, 255], "the bytes decide, not the name");
        let d = decode(&png(2, 2, |_, _| [1, 2, 3, 128]), "png").unwrap();
        assert!(d.has_alpha);
        assert!(is_decodable("PNG") && is_decodable("webp") && !is_decodable("dds") && !is_decodable("txt"));
    }

    #[test]
    fn garbage_and_unknown_formats_are_errors_not_panics() {
        assert!(decode(b"definitely not a picture", "png").is_err());
        assert!(decode(&[], "png").is_err());
        assert!(decode(b"abc", "xyz").unwrap_err().contains("xyz"));
        let mut cut = png(8, 8, |_, _| [9, 9, 9, 255]);
        cut.truncate(cut.len() / 2);
        assert!(decode(&cut, "png").is_err(), "a truncated file is refused");
    }

    #[test]
    fn a_mip_chain_has_one_level_per_halving_down_to_one_pixel() {
        assert_eq!((mip_levels(1, 1), mip_levels(2, 2), mip_levels(256, 256), mip_levels(300, 20)), (1, 2, 9, 9));
        let d = Decoded::solid(5, 3, [10, 20, 30, 255]);
        let (all, levels) = with_mips(&d, false);
        assert_eq!(levels, 3);
        // 5x3, 2x1, 1x1.
        assert_eq!(all.len(), (5 * 3 + 2 * 1 + 1) * 4);
        assert_eq!(&all[all.len() - 4..], &[10, 20, 30, 255], "a flat picture stays flat down to the last level");
    }

    #[test]
    fn colour_pictures_are_averaged_in_linear_light_and_data_pictures_as_they_are() {
        // A black and a white pixel side by side: the average of the light is brighter than the average of the numbers.
        let d = Decoded { width: 2, height: 1, rgba: vec![0, 0, 0, 255, 255, 255, 255, 255], has_alpha: false };
        let (colour, _) = with_mips(&d, true);
        let (data, _) = with_mips(&d, false);
        assert_eq!(data[8], 128, "a normal map or roughness map: the plain average");
        assert!(colour[8] > 180 && colour[8] < 195, "sRGB: 0.5 of the light is about 188, got {}", colour[8]);
    }

    #[test]
    fn see_through_pixels_do_not_bleed_their_colour_into_the_mips() {
        // Left pixel: red but invisible. Right: green and opaque. The mip must be green (at half opacity), not brown.
        let d = Decoded { width: 2, height: 1, rgba: vec![255, 0, 0, 0, 0, 255, 0, 255], has_alpha: true };
        let (mips, _) = with_mips(&d, true);
        let top = &mips[8..12];
        assert!(top[1] > 200 && top[0] < 40, "{top:?}");
        assert!((i32::from(top[3]) - 64).abs() < 3 || (i32::from(top[3]) - 128).abs() < 3, "alpha is averaged: {top:?}");
    }

    #[test]
    fn roughness_and_metallic_maps_are_combined_into_the_green_and_blue_channels() {
        let rough = Decoded::solid(2, 2, [200, 0, 0, 255]);
        let metal = Decoded::solid(2, 2, [90, 0, 0, 255]);
        let both = metallic_roughness(Some(&rough), Some(&metal)).unwrap();
        assert_eq!(both.pixel(1, 1), [255, 200, 90, 255]);
        let only_rough = metallic_roughness(Some(&rough), None).unwrap();
        assert_eq!(only_rough.pixel(0, 0), [255, 200, 255, 255], "a missing map leaves the material's own number alone");
        assert!(metallic_roughness(None, None).is_none());
        // Different sizes meet at the larger one.
        let big = Decoded::solid(4, 4, [10, 0, 0, 255]);
        let mixed = metallic_roughness(Some(&rough), Some(&big)).unwrap();
        assert_eq!((mixed.width, mixed.height), (4, 4));
    }

    #[test]
    fn colour_maps_are_srgb_and_data_maps_linear_and_the_sampler_follows_the_material() {
        let d = Decoded::solid(4, 4, [1, 2, 3, 255]);
        let colour = to_image(&d, true, &SamplerSettings::default());
        let data = to_image(&d, false, &SamplerSettings::default());
        assert_eq!(colour.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
        assert_eq!(data.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(colour.texture_descriptor.mip_level_count, 3);
        assert_eq!(colour.data.as_ref().unwrap().len(), (16 + 4 + 1) * 4);
        let nearest = to_image(&d, true, &SamplerSettings { filter: FilterKind::Nearest, address: AddressKind::Clamp, anisotropy: 8 });
        assert_eq!(nearest.texture_descriptor.mip_level_count, 1);
        let ImageSampler::Descriptor(s) = &nearest.sampler else { panic!("a descriptor") };
        assert_eq!((s.mag_filter, s.address_mode_u, s.anisotropy_clamp), (ImageFilterMode::Nearest, ImageAddressMode::ClampToEdge, 1));
        let ImageSampler::Descriptor(s) = &colour.sampler else { panic!("a descriptor") };
        assert_eq!((s.min_filter, s.mipmap_filter, s.address_mode_v, s.anisotropy_clamp), (ImageFilterMode::Linear, ImageFilterMode::Linear, ImageAddressMode::Repeat, 8));
    }
}
