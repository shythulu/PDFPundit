//! Markdown export (feature `export`, D-024).
//!
//! [`layouts`] reads a document's pages, fonts and links and runs T-32a's
//! layout analysis on each page; [`markdown::to_markdown`] writes the
//! Markdown. Both are pure: the export job in `jobs.rs` does the file I/O.

pub(crate) mod layout;
pub(crate) mod links;
pub(crate) mod markdown;

use sha2::{Digest, Sha256};

use crate::engine::ExtractedImage;
use crate::pdf::model::Finding;
use crate::pdf::text::{ExtractError, ExtractOptions, extract_text, font_table};

use layout::{Block, PageLayout, analyse};

/// Every page of `pdf` laid out, in page order. `findings` are the
/// document's (only each page's own become its notes); `images` are the
/// repair's extracted images (`RepairOutcome.images`), each placed on the
/// page its name gives (`p<page from 1>-<n>.<ext>`). An image whose name
/// gives no page of the document follows the last page's blocks.
pub(crate) fn layouts(
    pdf: &[u8],
    findings: &[Finding],
    images: &[(String, Vec<u8>)],
) -> Result<Vec<PageLayout>, ExtractError> {
    let pages = extract_text(pdf, &ExtractOptions::default())?;
    let fonts = font_table(pdf)?;
    let links = links::uri_annots(pdf);
    let images: Vec<(Option<u32>, ExtractedImage)> = images
        .iter()
        .map(|(name, bytes)| {
            let image = ExtractedImage {
                name: name.clone(),
                sha256: Sha256::digest(bytes).into(),
                len: bytes.len() as u64,
            };
            (image_page(name), image)
        })
        .collect();
    let mut out: Vec<PageLayout> = pages
        .iter()
        .map(|page| {
            let own: Vec<ExtractedImage> = images
                .iter()
                .filter(|(p, _)| *p == Some(page.index))
                .map(|(_, i)| i.clone())
                .collect();
            let annots = links
                .get(page.index as usize)
                .map_or(&[][..], Vec::as_slice);
            analyse(page, &fonts, findings, &own, annots)
        })
        .collect();
    if let Some(last) = out.last_mut() {
        let count = pages.len() as u64;
        last.blocks.extend(
            images
                .iter()
                .filter(|(p, _)| p.is_none_or(|p| u64::from(p) >= count))
                .map(|(_, i)| Block::Image {
                    name: i.name.clone(),
                    sha256: i.sha256,
                    len: i.len,
                }),
        );
    }
    Ok(out)
}

/// The 0-based page an extracted image's name places it on: `p3-1.jpg` is
/// page index 2. `None` for any other name.
fn image_page(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('p')?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || !rest[digits..].starts_with('-') {
        return None;
    }
    rest[..digits].parse::<u32>().ok()?.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::fixtures::{GOLDEN_TEXT, TINY_JPEG, golden_pdf};

    #[test]
    fn an_image_name_gives_its_page() {
        assert_eq!(image_page("p3-1.jpg"), Some(2));
        assert_eq!(image_page("p1-12.jpg"), Some(0));
        for name in [
            "p0-1.jpg",
            "p-1.jpg",
            "x3-1.jpg",
            "p3.jpg",
            "p99999999999-1.jpg",
        ] {
            assert_eq!(image_page(name), None, "{name}");
        }
    }

    #[test]
    fn images_land_on_their_page_and_strays_after_the_last() {
        let images = vec![
            ("p1-1.jpg".to_owned(), TINY_JPEG.to_vec()),
            ("orphan-1.jpg".to_owned(), b"x".to_vec()),
            ("p9-1.jpg".to_owned(), b"y".to_vec()),
        ];
        let pages = layouts(&golden_pdf(), &[], &images).expect("loads");
        assert_eq!(pages.len(), GOLDEN_TEXT.len());
        let names = |p: &PageLayout| -> Vec<String> {
            p.blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Image { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(names(&pages[0]), ["p1-1.jpg"]);
        assert_eq!(names(&pages[1]), ["orphan-1.jpg", "p9-1.jpg"]);
        let Some(Block::Image { sha256, len, .. }) = pages[0].blocks.last() else {
            panic!("page one ends with its image");
        };
        assert_eq!(*len, TINY_JPEG.len() as u64);
        assert_eq!(*sha256, <[u8; 32]>::from(Sha256::digest(TINY_JPEG)));
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        assert!(layouts(b"not a pdf", &[], &[]).is_err());
    }
}
