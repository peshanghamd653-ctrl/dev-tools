//! Putting the screenshot on the clipboard — the step that stands in for an
//! image upload.
//!
//! GitHub documents no API for attaching an image to an issue (see the crate
//! docs), so the handoff is the user pasting into the issue body in their
//! browser. That flow has a constraint worth stating rather than discovering:
//! **on Windows the clipboard contents belong to the process that set them
//! and are cleared when it exits.** Pasting therefore has to happen while
//! DevOS is still running — which is the expected flow, since the user goes
//! from the capture straight to the issue GitHub has just opened for them —
//! but closing DevOS first loses the image.

use std::borrow::Cow;
use std::path::Path;

use arboard::{Clipboard, ImageData};
use xcap::image::ImageReader;

use crate::{IssueError, IssueResult};

/// Put the PNG at `path` on the clipboard as an image, not as a file path —
/// a browser text box accepts the pixels, not a reference to them.
///
/// Blocking, like [`crate::capture_primary`]: the clipboard is a synchronous
/// OS API and the decode is CPU work on a full-screen image.
pub fn copy_image(path: &Path) -> IssueResult<()> {
    let (width, height, rgba) = decode_rgba(path)?;
    let mut clipboard = Clipboard::new().map_err(|e| IssueError::Clipboard(e.to_string()))?;
    clipboard
        .set_image(ImageData {
            width,
            height,
            bytes: Cow::Owned(rgba),
        })
        .map_err(|e| IssueError::Clipboard(e.to_string()))
}

/// Width, height, and raw RGBA8 — the only form `arboard` takes. Split out
/// from [`copy_image`] so the half that can be tested without an OS clipboard
/// is tested: a headless or locked session has no clipboard to write to, but
/// decoding is pure.
fn decode_rgba(path: &Path) -> IssueResult<(usize, usize, Vec<u8>)> {
    let reader = ImageReader::open(path)
        .map_err(|e| IssueError::Clipboard(format!("{}: {e}", path.display())))?
        // Sniff the content instead of trusting the extension: the file is
        // named by this crate, but the path arrives back from the frontend.
        .with_guessed_format()
        .map_err(|e| IssueError::Clipboard(format!("{}: {e}", path.display())))?;
    let image = reader
        .decode()
        .map_err(|e| IssueError::Clipboard(format!("{}: {e}", path.display())))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Ok((width as usize, height as usize, image.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcap::image::{Rgba, RgbaImage};

    /// Writes a real PNG, so the decode under test is a real decode.
    fn write_png(path: &Path, width: u32, height: u32) {
        let mut image = RgbaImage::new(width, height);
        image.put_pixel(0, 0, Rgba([12, 34, 56, 255]));
        image.save(path).unwrap();
    }

    #[test]
    fn a_captured_png_decodes_to_rgba_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("shot-1754400000000.png");
        write_png(&path, 7, 5);

        let (width, height, bytes) = decode_rgba(&path).expect("decode");

        assert_eq!((width, height), (7, 5));
        assert_eq!(
            bytes.len(),
            7 * 5 * 4,
            "arboard wants four bytes per pixel, un-premultiplied RGBA"
        );
        assert_eq!(
            &bytes[..4],
            &[12, 34, 56, 255],
            "channel order is RGBA, not BGRA"
        );
    }

    #[test]
    fn a_missing_file_is_a_clipboard_error_not_a_panic() {
        let temp = tempfile::tempdir().unwrap();
        let error = decode_rgba(&temp.path().join("never-captured.png")).unwrap_err();

        assert!(
            matches!(error, IssueError::Clipboard(_)),
            "expected a clipboard error, got {error:?}"
        );
        assert!(
            error.to_string().contains("never-captured.png"),
            "the message must name the file: {error}"
        );
    }

    #[test]
    fn a_file_that_is_not_an_image_is_rejected_by_content_not_by_name() {
        let temp = tempfile::tempdir().unwrap();
        // Named like a capture, so only sniffing the bytes catches it.
        let path = temp.path().join("shot-1754400000000.png");
        std::fs::write(&path, b"<html>not a screenshot</html>").unwrap();

        let error = decode_rgba(&path).unwrap_err();

        assert!(
            matches!(error, IssueError::Clipboard(_)),
            "expected a clipboard error, got {error:?}"
        );
    }
}
