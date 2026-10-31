//! The images of a game's launch presentation: the application icon and the splash screen.
//!
//! Both are ordinary project files (PNG or JPEG) that the manifest refers to by their path in the project (see
//! `manifest::LaunchSettings`). This module says whether a file is usable for its job, with a message that says what
//! is wrong, and decodes it for the runtime; the editor, `forge check`, the build and the player all use the same
//! checks, so they cannot disagree about what works.

use std::io::Cursor;

use super::{file_name, Project};

/// The image formats the engine reads for these (what the `image` crate is built with here).
pub const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];
/// An image bigger than this (in bytes, or on a side in pixels) is refused: it would take long to read and a window
/// icon or a splash screen never needs it.
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_IMAGE_SIDE: u32 = 8192;
/// The smallest icon: below this the system has nothing to draw.
pub const MIN_ICON_SIDE: u32 = 16;

/// What an image is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Icon,
    Splash,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Icon => "icon",
            Role::Splash => "splash screen",
        }
    }
}

/// What was found out about a usable image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

/// Whether the file name says it is an image of a supported format.
pub fn has_supported_extension(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Checks the contents of an image file meant for `role`. `name` is only used in the messages.
pub fn check_image(name: &str, bytes: &[u8], role: Role) -> Result<ImageInfo, String> {
    if !has_supported_extension(name) {
        let ext = name.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_else(|| "without an extension".to_owned());
        return Err(format!("'{name}' is not a supported image ({ext}): the {} must be a PNG or JPEG file", role.label()));
    }
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!("'{name}' is too big for the {} ({} MiB; at most {} MiB)", role.label(), bytes.len() / (1024 * 1024), MAX_IMAGE_BYTES / (1024 * 1024)));
    }
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("'{name}' could not be read: {e}"))?;
    let (width, height) = reader.into_dimensions().map_err(|e| format!("'{name}' is not a valid PNG or JPEG image ({e})"))?;
    if width.max(height) > MAX_IMAGE_SIDE {
        return Err(format!("'{name}' is {width} x {height} pixels: too big for the {} (at most {MAX_IMAGE_SIDE} on a side)", role.label()));
    }
    if role == Role::Icon && width.min(height) < MIN_ICON_SIDE {
        return Err(format!("'{name}' is {width} x {height} pixels: too small for an icon (at least {MIN_ICON_SIDE} x {MIN_ICON_SIDE}, 256 x 256 or more is best)"));
    }
    Ok(ImageInfo { width, height })
}

/// Something worth telling about an image that is still accepted: an icon that is not square is squeezed into one.
pub fn advice(info: ImageInfo, role: Role) -> Option<String> {
    (role == Role::Icon && info.width != info.height).then(|| format!("The icon is {} x {} pixels, not square: it will look squeezed. A square image (256 x 256 or more) is best.", info.width, info.height))
}

/// Checks a project file chosen as the icon or the splash screen: that it is there, and then [`check_image`].
pub fn check_project_file(project: &Project, rel: &str, role: Role) -> Result<ImageInfo, String> {
    let path = project.abs(rel).map_err(|e| format!("{e}"))?;
    if !path.is_file() {
        return Err(format!("the {} '{rel}' is missing from the project", role.label()));
    }
    let size = path.metadata().map(|m| m.len()).unwrap_or(0);
    if size > MAX_IMAGE_BYTES {
        return Err(format!("'{}' is too big for the {} ({} MiB; at most {} MiB)", file_name(rel), role.label(), size / (1024 * 1024), MAX_IMAGE_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("the {} '{rel}' could not be read: {e}", role.label()))?;
    check_image(rel, &bytes, role)
}

/// The image as 8-bit RGBA pixels: (width, height, bytes). What the runtime hands to the window and the GPU.
pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let image = image::load_from_memory(bytes).map_err(|e| format!("{e}"))?.into_rgba8();
    let (w, h) = image.dimensions();
    Ok((w, h, image.into_raw()))
}

/// The icon as RGBA pixels for the window: a big picture is brought down to at most 256 x 256 (a window icon is never
/// drawn bigger, and the system may refuse a huge one), squeezed to a square if it is not one.
pub fn decode_icon(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    const ICON_SIDE: u32 = 256;
    let image = image::load_from_memory(bytes).map_err(|e| format!("{e}"))?.into_rgba8();
    let (w, h) = image.dimensions();
    if w == h && w <= ICON_SIDE {
        return Ok((w, h, image.into_raw()));
    }
    let side = w.max(h).min(ICON_SIDE);
    let resized = image::imageops::resize(&image, side, side, image::imageops::FilterType::Triangle);
    Ok((side, side, resized.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real PNG of `w` x `h` pixels.
    pub fn png(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 60, 60, 255]));
        let mut out = Vec::new();
        image.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn a_square_png_is_a_good_icon_and_any_png_a_good_splash() {
        assert_eq!(check_image("icon.png", &png(64, 64), Role::Icon), Ok(ImageInfo { width: 64, height: 64 }));
        assert_eq!(check_image("Assets/Splash.PNG", &png(320, 180), Role::Splash), Ok(ImageInfo { width: 320, height: 180 }), "the extension is read in any case");
        assert_eq!(advice(ImageInfo { width: 64, height: 64 }, Role::Icon), None);
        assert!(advice(ImageInfo { width: 320, height: 180 }, Role::Icon).unwrap().contains("not square"));
        assert_eq!(advice(ImageInfo { width: 320, height: 180 }, Role::Splash), None, "a splash screen may be any shape");
    }

    #[test]
    fn what_is_wrong_with_an_image_is_said_in_words() {
        let bmp = check_image("logo.bmp", &png(32, 32), Role::Icon).unwrap_err();
        assert!(bmp.contains("PNG or JPEG") && bmp.contains(".bmp") && bmp.contains("icon"), "{bmp}");
        let none = check_image("logo", &[], Role::Splash).unwrap_err();
        assert!(none.contains("without an extension") && none.contains("splash screen"), "{none}");
        let junk = check_image("logo.png", b"this is not an image", Role::Icon).unwrap_err();
        assert!(junk.contains("not a valid"), "{junk}");
        let tiny = check_image("logo.png", &png(8, 8), Role::Icon).unwrap_err();
        assert!(tiny.contains("too small"), "{tiny}");
        assert!(check_image("logo.png", &png(8, 8), Role::Splash).is_ok(), "only an icon has a smallest size");
        let huge = vec![0u8; MAX_IMAGE_BYTES as usize + 1];
        assert!(check_image("big.png", &huge, Role::Splash).unwrap_err().contains("too big"));
    }

    #[test]
    fn a_window_icon_is_square_and_no_bigger_than_256() {
        assert_eq!(decode_icon(&png(64, 64)).map(|(w, h, p)| (w, h, p.len())), Ok((64, 64, 64 * 64 * 4)));
        assert_eq!(decode_icon(&png(512, 512)).map(|(w, h, _)| (w, h)), Ok((256, 256)), "brought down");
        assert_eq!(decode_icon(&png(400, 100)).map(|(w, h, _)| (w, h)), Ok((256, 256)), "squeezed into a square");
        assert_eq!(decode_icon(&png(100, 40)).map(|(w, h, _)| (w, h)), Ok((100, 100)));
    }

    #[test]
    fn pixels_come_out_as_rgba() {
        let (w, h, rgba) = decode_rgba(&png(4, 2)).unwrap();
        assert_eq!((w, h, rgba.len()), (4, 2, 4 * 2 * 4));
        assert_eq!(&rgba[..4], &[200, 60, 60, 255]);
        assert!(decode_rgba(b"nope").is_err());
    }

    #[test]
    fn a_project_file_is_checked_where_it_is() {
        let dir = std::env::temp_dir().join(format!("forge_launch_test_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let project = Project::create(&dir, "Launch Test").unwrap();
        assert!(check_project_file(&project, "Assets/Textures/icon.png", Role::Icon).unwrap_err().contains("missing"));
        std::fs::write(dir.join("Assets/Textures/icon.png"), png(32, 32)).unwrap();
        assert_eq!(check_project_file(&project, "Assets/Textures/icon.png", Role::Icon), Ok(ImageInfo { width: 32, height: 32 }));
        std::fs::write(dir.join("Assets/Textures/icon.txt"), b"x").unwrap();
        assert!(check_project_file(&project, "Assets/Textures/icon.txt", Role::Icon).unwrap_err().contains("PNG or JPEG"));
        assert!(check_project_file(&project, "../outside.png", Role::Icon).is_err(), "a path that leaves the project is refused");
        std::fs::remove_dir_all(&dir).ok();
    }
}
