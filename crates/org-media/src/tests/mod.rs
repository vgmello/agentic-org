//! Media allowlist, filename classification, magic-byte sniffing, and (under `sanitize`) image
//! sanitization and document validation (`TASK-98.1`, `TASK-252`).

#[cfg(feature = "sanitize")]
mod clean;
#[cfg(feature = "sanitize")]
mod clean_streams;
#[cfg(feature = "sanitize")]
mod decode_gate;

use std::path::Path;

use org_proto::ErrorCode;

use crate::{
    check_filename, check_kept_filename, classify_filename, sniff, AllowedMedia, MediaType,
};

#[test]
fn blocked_types_and_declared_mime_parsing_enforce_allowlist() {
    for blocked in [
        "multipart/form-data",
        "text/html",
        "application/xhtml+xml",
        "image/svg+xml",
        "application/javascript",
        "text/javascript",
        "application/ecmascript",
        "application/octet-stream",
        "application/x-msdownload",
        "application/x-executable",
        "application/x-elf",
        "application/x-mach-binary",
        "image/tiff",
        "",
    ] {
        assert_eq!(
            MediaType::parse_declared(blocked).unwrap_err().code,
            ErrorCode::Unsupported,
            "{blocked}"
        );
    }
    assert_eq!(
        MediaType::parse_declared("Image/PNG").unwrap(),
        MediaType::Png
    );
    assert_eq!(
        MediaType::parse_declared("text/plain; charset=UTF-8").unwrap(),
        MediaType::PlainText
    );
    assert!(MediaType::parse_declared("image/png; charset=utf-8").is_err());
    assert!(MediaType::parse_declared("text/plain; charset=latin1").is_err());
    assert_eq!(AllowedMedia::from_extension("JPG"), Some(MediaType::Jpeg));
    assert_eq!(AllowedMedia::from_extension("exe"), None);
}

#[test]
fn classify_and_check_filename_reject_hidden_traversal_and_unlisted_extensions() {
    assert_eq!(
        check_filename(" photo.JPG ", MediaType::Jpeg).unwrap(),
        "photo.JPG"
    );
    assert!(check_filename("report.pdf", MediaType::Pdf).is_ok());
    assert!(check_kept_filename("photo.png", MediaType::Jpeg).is_ok());
    assert!(check_kept_filename(" photo.png ", MediaType::Jpeg).is_err());

    let (name, media) = classify_filename(Path::new("/work/spec.MD")).unwrap();
    assert_eq!(name, "spec.MD");
    assert_eq!(media, MediaType::Markdown);

    for bad in [
        "",
        "../etc/passwd.pdf",
        "a/b.pdf",
        "a\\b.pdf",
        ".pdf",
        "x\u{0}.pdf",
        "nul\n.pdf",
    ] {
        assert!(check_filename(bad, MediaType::Pdf).is_err(), "{bad:?}");
    }
    assert_eq!(
        check_filename("run.exe", MediaType::PlainText)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        classify_filename(Path::new("/work/.hidden.json"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        classify_filename(Path::new("/work/script.sh"))
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert!(check_filename("notes.md", MediaType::Json).is_err());
    let long = format!("{}.txt", "a".repeat(260));
    assert!(check_filename(&long, MediaType::PlainText).is_err());
    assert_eq!(
        classify_filename(Path::new(&long)).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn sniff_rejects_executables_html_svg_and_mismatched_magic_without_codecs() {
    let elf = b"\x7fELF\x02\x01\x01\x00rest-of-binary".to_vec();
    let macho = vec![0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1];
    let pe = {
        let mut pe = b"MZ".to_vec();
        pe.resize(0x40, 0);
        pe[0x3c] = 0x40;
        pe.extend_from_slice(b"PE\0\0rest");
        pe
    };
    for binary in [elf, macho, pe] {
        assert_eq!(
            sniff::check_matches(MediaType::PlainText, &binary)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            sniff::check_matches(MediaType::Png, &binary)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }

    // Text starting with "MZ" (not a PE header) stays plain text.
    assert!(sniff::check_matches(MediaType::PlainText, b"MZ is a postcode prefix").is_ok());

    // HTML / SVG disguised behind BOM, comments, or declarations.
    for disguised in [
        "<!-- x --><html><script>alert(1)</script></html>",
        "\n  <!-- one -->\n<!-- two -->\n<body onload=alert(1)>",
        "<!-- a --!><script>alert(1)</script> -->",
        "<!--><script>alert(1)</script>",
        "<!---><script>alert(1)</script>",
        "<?xml x><script>alert(1)</script>?>",
        "<!x><iframe src=x>",
        "\u{feff}<!-- bom --><HTML>",
        "<!DOCTYPE html><script>x</script>",
        "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>",
        "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x\"><svg><script/></svg>",
    ] {
        assert_eq!(
            sniff::check_matches(MediaType::Markdown, disguised.as_bytes())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported,
            "{disguised}"
        );
    }

    // Text starting with an innocent or unclosed comment stays text.
    for text in [
        "<!-- toc -->\n# Title\n\nBody.\n",
        "<!-- never closed, so <script> is inside it",
    ] {
        assert!(
            sniff::check_matches(MediaType::Markdown, text.as_bytes()).is_ok(),
            "{text}"
        );
    }

    // Many tiny comments are walked linearly in one pass.
    let many = "<!---->".repeat(200_000) + "<html>";
    assert_eq!(
        sniff::check_matches(MediaType::PlainText, many.as_bytes())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );

    // Valid magic matches declared types; cross-type mismatches fail closed.
    assert!(sniff::check_matches(MediaType::Png, b"\x89PNG\r\n\x1a\nrest").is_ok());
    assert!(sniff::check_matches(MediaType::Jpeg, &[0xff, 0xd8, 0xff, 0xe0]).is_ok());
    assert!(sniff::check_matches(MediaType::Gif, b"GIF89a...").is_ok());
    assert!(sniff::check_matches(MediaType::Webp, b"RIFF\x00\x00\x00\x00WEBP").is_ok());
    assert!(sniff::check_matches(MediaType::Pdf, b"%PDF-1.7\n").is_ok());
    assert!(sniff::check_matches(MediaType::Docx, b"PK\x03\x04...").is_ok());
    assert_eq!(
        sniff::check_matches(MediaType::Png, &[0xff, 0xd8, 0xff, 0xe0])
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        sniff::check_matches(MediaType::Pdf, b"\x89PNG\r\n\x1a\n")
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        sniff::check_matches(MediaType::Json, b"%PDF-1.7\n")
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[cfg(feature = "sanitize")]
mod sanitize_tests {
    use org_proto::{ApiError, ErrorCode};

    use crate::image::{
        model_derivative, BoundedVecWriter, DERIVATIVE_MAX_BYTES, DERIVATIVE_MAX_EDGE,
    };
    use crate::testkit::{
        content_types, docx, entry, gif, gif_frame_count, gif_frames, jpeg_with_exif, pdf,
        pdf_with_object_stream, pdf_with_object_stream_named, png, png_header_only, png_with_text,
        webp, xlsx, zip, GifRect, ZipEntry, GPS_MARKER,
    };
    use crate::{validate, MediaType, ValidatedFile};

    fn code(result: Result<ValidatedFile, ApiError>) -> ErrorCode {
        result.expect_err("refused").code
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn exif_and_gps_are_stripped_and_the_orientation_is_applied_before_hashing() {
        let raw = jpeg_with_exif(40, 20, 6, GPS_MARKER);
        assert!(contains(&raw, GPS_MARKER.as_bytes()));
        assert!(contains(&raw, b"Exif"));
        let kept = validate(MediaType::Jpeg, raw.clone()).unwrap();
        assert_eq!(kept.media, MediaType::Jpeg);
        assert!(!contains(&kept.bytes, GPS_MARKER.as_bytes()));
        assert!(!contains(&kept.bytes, b"Exif"));
        // Orientation 6 turns the 40x20 picture a quarter: the kept pixels are 20x40.
        assert_eq!((kept.width, kept.height), (Some(20), Some(40)));
        let decoded = ::image::load_from_memory(&kept.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (20, 40));
        // The kept bytes differ from the raw upload so hashing kept bytes never hashes EXIF.
        assert_ne!(kept.bytes, raw);
    }

    #[test]
    fn png_text_chunks_are_dropped_and_a_png_stays_a_png() {
        let raw = png_with_text(16, 16, GPS_MARKER);
        let kept = validate(MediaType::Png, raw).unwrap();
        assert_eq!(kept.media, MediaType::Png);
        assert!(!contains(&kept.bytes, GPS_MARKER.as_bytes()));
        assert!(!contains(&kept.bytes, b"tEXt"));
        assert_eq!((kept.width, kept.height), (Some(16), Some(16)));
        // Sanitizing again changes nothing: the kept bytes are canonical.
        let again = validate(MediaType::Png, kept.bytes.clone()).unwrap();
        assert_eq!(again.bytes, kept.bytes);
    }

    #[test]
    fn webp_and_gif_are_kept_in_their_formats() {
        let kept = validate(MediaType::Webp, webp(12, 8)).unwrap();
        assert_eq!(kept.media, MediaType::Webp);
        assert_eq!((kept.width, kept.height), (Some(12), Some(8)));
        let animated = validate(MediaType::Gif, gif(8, 8, 3)).unwrap();
        assert_eq!(animated.media, MediaType::Gif);
        assert_eq!(gif_frame_count(&animated.bytes), 3);
        let still = validate(MediaType::Gif, gif(8, 8, 1)).unwrap();
        assert_eq!(gif_frame_count(&still.bytes), 1);
    }

    #[test]
    fn oversized_pixel_bombs_and_frame_bombs_are_refused() {
        // Over 16384 on a side: refused from the header, before any pixel is decoded.
        assert_eq!(
            code(validate(MediaType::Png, png_header_only(20_000, 10))),
            ErrorCode::TooLarge
        );
        // Within each side but over 64 MP.
        assert_eq!(
            code(validate(MediaType::Png, png_header_only(9_000, 9_000))),
            ErrorCode::TooLarge
        );
        // 201 frames.
        assert_eq!(
            code(validate(MediaType::Gif, gif(2, 2, 201))),
            ErrorCode::TooLarge
        );
        assert!(validate(MediaType::Gif, gif(2, 2, 200)).is_ok());
        // Over 10 MiB raw.
        let mut big = png(4, 4);
        big.resize(10 * 1024 * 1024 + 1, 0);
        assert_eq!(code(validate(MediaType::Png, big)), ErrorCode::TooLarge);
        // A damaged image.
        let mut broken = png(8, 8);
        broken.truncate(40);
        assert_eq!(
            code(validate(MediaType::Png, broken)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn the_model_derivative_is_at_most_1568_pixels_and_1_mib() {
        let kept = validate(MediaType::Png, png(3_000, 1_000)).unwrap();
        let derivative = model_derivative(MediaType::Png, &kept.bytes).unwrap();
        assert_eq!(derivative.media, MediaType::Jpeg);
        assert_eq!(derivative.width, DERIVATIVE_MAX_EDGE);
        assert!(derivative.height <= DERIVATIVE_MAX_EDGE);
        assert!(derivative.bytes.len() <= DERIVATIVE_MAX_BYTES);
        let decoded = ::image::load_from_memory(&derivative.bytes).unwrap();
        assert_eq!(decoded.width(), DERIVATIVE_MAX_EDGE);
        // A small image is not enlarged.
        let small = model_derivative(MediaType::Png, &png(30, 10)).unwrap();
        assert_eq!((small.width, small.height), (30, 10));
        // Only images have one.
        assert_eq!(
            model_derivative(MediaType::Pdf, &pdf(1)).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn a_pdf_is_checked_and_its_pages_counted() {
        let kept = validate(MediaType::Pdf, pdf(3)).unwrap();
        assert_eq!(kept.page_count, Some(3));
        assert_eq!(kept.media, MediaType::Pdf);
        let compressed = validate(MediaType::Pdf, pdf_with_object_stream(7)).unwrap();
        assert_eq!(compressed.page_count, Some(7));
        // Truncated: no `%%EOF`.
        let mut truncated = pdf(2);
        truncated.truncate(truncated.len() - 8);
        assert_eq!(
            code(validate(MediaType::Pdf, truncated)),
            ErrorCode::InvalidRequest
        );
        // `startxref` outside the file.
        let text = String::from_utf8(pdf(1)).unwrap();
        let (head, _) = text.split_once("startxref").unwrap();
        let lying = format!("{head}startxref\n999999999\n%%EOF\n");
        assert_eq!(
            code(validate(MediaType::Pdf, lying.into_bytes())),
            ErrorCode::InvalidRequest
        );
        // No page tree at all.
        let empty =
            b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\nstartxref\n9\n%%EOF\n".to_vec();
        assert_eq!(
            code(validate(MediaType::Pdf, empty)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn office_documents_are_checked_without_unpacking_them() {
        assert!(validate(MediaType::Docx, docx())
            .unwrap()
            .page_count
            .is_none());
        assert!(validate(MediaType::Xlsx, xlsx()).is_ok());
        // A workbook declared as a document, and the other way round.
        assert_eq!(
            code(validate(MediaType::Docx, xlsx())),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(validate(MediaType::Xlsx, docx())),
            ErrorCode::Unsupported
        );
        // A plain ZIP is not a document.
        let plain = zip(&[entry("readme.txt", b"hello")]);
        assert_eq!(
            code(validate(MediaType::Docx, plain)),
            ErrorCode::Unsupported
        );
        // Macros.
        let types = content_types("wordprocessingml.document");
        let macros = zip(&[
            entry("[Content_Types].xml", types.as_bytes()),
            entry("word/document.xml", b"<w:document/>"),
            entry("word/vbaProject.bin", b"\x00\x01"),
        ]);
        assert_eq!(
            code(validate(MediaType::Docx, macros)),
            ErrorCode::Unsupported
        );
        // A zip bomb's declared size is refused without inflating anything.
        let bomb = zip(&[
            entry("[Content_Types].xml", types.as_bytes()),
            entry("word/document.xml", b"<w:document/>"),
            ZipEntry {
                name: "word/media/huge.bin",
                data: b"0123456789",
                declared: Some(300 * 1024 * 1024),
            },
        ]);
        assert_eq!(
            code(validate(MediaType::Docx, bomb)),
            ErrorCode::Unsupported
        );
        // Path traversal in an entry name.
        let traversal = zip(&[
            entry("[Content_Types].xml", types.as_bytes()),
            entry("word/document.xml", b"<w:document/>"),
            entry("../../etc/passwd", b"x"),
        ]);
        assert_eq!(
            code(validate(MediaType::Docx, traversal)),
            ErrorCode::Unsupported
        );
        // Truncated: the end record is gone.
        let mut cut = docx();
        cut.truncate(cut.len() - 10);
        assert_eq!(
            code(validate(MediaType::Docx, cut)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn text_documents_must_be_utf8_without_nul_and_json_must_parse() {
        for (media, text) in [
            (MediaType::Markdown, "# Notes\n\n- one\n- two\n"),
            (MediaType::PlainText, "plain words, ünïcode ✓\n"),
            (MediaType::Csv, "a,b\n1,2\n"),
            (MediaType::Json, "{\"a\": [1, 2, {\"b\": null}]}"),
        ] {
            let kept = validate(media, text.as_bytes().to_vec()).unwrap();
            assert_eq!(kept.bytes, text.as_bytes(), "{media:?} is kept as uploaded");
        }
        assert_eq!(
            code(validate(MediaType::PlainText, vec![0xff, 0xfe, 0x41])),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(validate(MediaType::Csv, b"a,b\0\n".to_vec())),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(validate(MediaType::Json, b"{\"a\": }".to_vec())),
            ErrorCode::InvalidRequest
        );
        // Deep nesting is valid JSON, and checking it walks the document without recursion, so
        // a hostile depth cannot exhaust the stack; an unbalanced one is refused.
        let deep = "[".repeat(100_000) + &"]".repeat(100_000);
        assert!(validate(MediaType::Json, deep.into_bytes()).is_ok());
        let unbalanced = "[".repeat(100_000) + &"]".repeat(99_999);
        assert_eq!(
            code(validate(MediaType::Json, unbalanced.into_bytes())),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn validate_refuses_mismatched_magic_and_markup() {
        assert_eq!(
            code(validate(MediaType::Png, jpeg_with_exif(4, 4, 1, "x"))),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(validate(MediaType::Png, docx())),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(validate(MediaType::Pdf, png(4, 4))),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(validate(MediaType::Json, pdf(1))),
            ErrorCode::Unsupported
        );
        assert!(validate(MediaType::PlainText, b"MZ is a postcode prefix".to_vec()).is_ok());
    }

    #[test]
    fn a_gif_sub_frame_larger_than_its_canvas_is_refused_before_it_is_decompressed() {
        for side in [2_000, 12_200] {
            let frame = GifRect {
                left: 0,
                top: 0,
                width: side,
                height: side,
            };
            let bomb = gif_frames(1, 1, &[frame], false);
            assert_eq!(
                code(validate(MediaType::Gif, bomb)),
                ErrorCode::TooLarge,
                "{side}"
            );
        }
        let past_edge = GifRect {
            left: 0,
            top: 0,
            width: 9,
            height: 8,
        };
        assert_eq!(
            code(validate(
                MediaType::Gif,
                gif_frames(8, 8, &[past_edge], false)
            )),
            ErrorCode::TooLarge
        );
    }

    #[test]
    fn a_gif_frame_covering_part_of_its_canvas_still_fits_the_canvas_bounded_budget() {
        let frames = [
            GifRect {
                left: 0,
                top: 0,
                width: 400,
                height: 400,
            },
            GifRect {
                left: 100,
                top: 100,
                width: 200,
                height: 200,
            },
        ];
        let kept = validate(MediaType::Gif, gif_frames(400, 400, &frames, true)).unwrap();
        assert_eq!(kept.media, MediaType::Gif);
        assert_eq!((kept.width, kept.height), (Some(400), Some(400)));
        assert_eq!(gif_frame_count(&kept.bytes), 2);
    }

    #[test]
    fn an_encoder_sink_refuses_its_first_byte_past_the_cap_and_never_allocates_beyond_it() {
        use std::io::Write as _;
        let mut sink = BoundedVecWriter::new(10);
        sink.write_all(b"12345678").unwrap();
        assert!(!sink.overflowed());
        assert!(sink.write_all(b"abc").is_err());
        assert!(sink.overflowed());
        assert!(sink.write_all(b"9").is_err());
        let bytes = sink.into_inner();
        assert_eq!(bytes, b"12345678");
        assert!(bytes.capacity() <= 10, "{}", bytes.capacity());
    }

    #[test]
    fn an_object_stream_named_many_times_is_inflated_once() {
        let named = pdf_with_object_stream_named(7, 400, 200_000);
        let kept = validate(MediaType::Pdf, named).unwrap();
        assert_eq!(kept.page_count, Some(7));
    }

    fn patched(mut bytes: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
        assert_eq!(from.len(), to.len());
        let at = bytes
            .windows(from.len())
            .position(|w| w == from)
            .expect("present");
        bytes[at..at + to.len()].copy_from_slice(to);
        bytes
    }

    #[test]
    fn office_documents_hiding_macros_or_disagreeing_headers_are_refused() {
        let renamed = patched(docx(), b"word/document.xml", b"word/documenT.xml");
        assert_eq!(
            code(validate(MediaType::Docx, renamed)),
            ErrorCode::InvalidRequest
        );
        let types = content_types("wordprocessingml.document");
        let lying = zip(&[
            entry("[Content_Types].xml", types.as_bytes()),
            entry("word/document.xml", b"<w:document/>"),
            ZipEntry {
                name: "word/media/a.bin",
                data: b"0123456789",
                declared: Some(11),
            },
        ]);
        assert_eq!(
            code(validate(MediaType::Docx, lying)),
            ErrorCode::InvalidRequest
        );
        let sheet_types = content_types("spreadsheetml.sheet");
        let macro_sheet = zip(&[
            entry("[Content_Types].xml", sheet_types.as_bytes()),
            entry("xl/workbook.xml", b"<workbook/>"),
            entry("xl/macrosheets/sheet1.xml", b"<xm:macrosheet/>"),
        ]);
        assert_eq!(
            code(validate(MediaType::Xlsx, macro_sheet)),
            ErrorCode::Unsupported
        );
        for declared in [
            "application/vnd.ms-excel.macrosheet+xml",
            "application/vnd.ms-office.vbaProject",
        ] {
            let types = sheet_types.replace(
                "</Types>",
                &format!("<Override PartName=\"/x\" ContentType=\"{declared}\"/></Types>"),
            );
            let hidden = zip(&[
                entry("[Content_Types].xml", types.as_bytes()),
                entry("xl/workbook.xml", b"<workbook/>"),
            ]);
            assert_eq!(
                code(validate(MediaType::Xlsx, hidden)),
                ErrorCode::Unsupported,
                "{declared}"
            );
        }
    }
}
