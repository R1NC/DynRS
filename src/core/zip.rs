use flate2::{Compression, bufread, write};
use std::io::{self, BufRead, Read, Write};

const BUFFER_SIZE: usize = 16 * 1024; // zlib default chunk size

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionFormat {
    Gzip,
    Zlib,
    Raw,
}

pub fn compress<R: Read>(reader: R, format: CompressionFormat) -> io::Result<Vec<u8>> {
    let result = Vec::new();
    match format {
        CompressionFormat::Gzip => {
            let mut encoder = write::GzEncoder::new(result, Compression::default());
            process_stream(reader, &mut encoder)?;
            Ok(encoder.finish()?)
        }
        CompressionFormat::Zlib => {
            let mut encoder = write::ZlibEncoder::new(result, Compression::default());
            process_stream(reader, &mut encoder)?;
            Ok(encoder.finish()?)
        }
        CompressionFormat::Raw => {
            let mut encoder = write::DeflateEncoder::new(result, Compression::default());
            process_stream(reader, &mut encoder)?;
            Ok(encoder.finish()?)
        }
    }
}

pub fn decompress<R: Read + BufRead + 'static>(
    reader: R,
    format: CompressionFormat,
) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let decoder = match format {
        CompressionFormat::Gzip => Box::new(bufread::GzDecoder::new(reader)) as Box<dyn Read>,
        CompressionFormat::Zlib => Box::new(bufread::ZlibDecoder::new(reader)) as Box<dyn Read>,
        CompressionFormat::Raw => Box::new(bufread::DeflateDecoder::new(reader)) as Box<dyn Read>,
    };
    process_stream(decoder, &mut result)?;
    Ok(result)
}

fn process_stream<R: Read, W: Write>(mut reader: R, mut writer: W) -> io::Result<()> {
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buffer[..bytes_read])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const ALL_FORMATS: [CompressionFormat; 3] = [
        CompressionFormat::Gzip,
        CompressionFormat::Zlib,
        CompressionFormat::Raw,
    ];

    #[test]
    fn every_format_round_trips_repetitive_data() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(20);

        for format in ALL_FORMATS {
            let compressed = compress(Cursor::new(&data), format).expect("data compresses");
            assert!(
                compressed.len() < data.len(),
                "{format:?} should shrink repetitive data"
            );

            let restored = decompress(Cursor::new(compressed), format).expect("data decompresses");
            assert_eq!(restored, data, "{format:?} must restore the input");
        }
    }

    #[test]
    fn format_headers_are_distinct() {
        let data = b"header check".to_vec();

        let gzip = compress(Cursor::new(data.clone()), CompressionFormat::Gzip).unwrap();
        assert_eq!(&gzip[..2], &[0x1f, 0x8b][..], "gzip magic bytes");

        let zlib = compress(Cursor::new(data.clone()), CompressionFormat::Zlib).unwrap();
        assert_eq!(zlib[0] & 0x0f, 8, "zlib CMF advertises deflate");
        assert_eq!(
            ((zlib[0] as u16) << 8 | zlib[1] as u16) % 31,
            0,
            "zlib header carries a check value"
        );

        let raw = compress(Cursor::new(data), CompressionFormat::Raw).unwrap();
        assert_ne!(
            &raw[..2],
            &[0x1f, 0x8b][..],
            "raw deflate has no gzip header"
        );
    }

    #[test]
    fn empty_and_binary_payloads_round_trip() {
        for format in ALL_FORMATS {
            // An empty input still produces a valid, non-empty stream.
            let compressed =
                compress(Cursor::new(Vec::new()), format).expect("empty data compresses");
            assert!(
                !compressed.is_empty(),
                "{format:?} emits a stream even for no input"
            );
            let restored =
                decompress(Cursor::new(compressed), format).expect("empty stream decompresses");
            assert!(restored.is_empty(), "{format:?} restores an empty payload");

            // Every byte value, and enough data to cross the internal buffer size.
            let binary: Vec<u8> = (0..=255u8).cycle().take(BUFFER_SIZE * 3 + 7).collect();
            let compressed =
                compress(Cursor::new(binary.clone()), format).expect("binary data compresses");
            let restored =
                decompress(Cursor::new(compressed), format).expect("binary data decompresses");
            assert_eq!(restored, binary, "{format:?} must restore binary data");
        }
    }

    #[test]
    fn a_decompressor_rejects_the_wrong_format() {
        let gzip = compress(Cursor::new(b"payload".to_vec()), CompressionFormat::Gzip).unwrap();

        // gzip data has a header that neither of the other two formats accepts.
        assert!(decompress(Cursor::new(gzip.clone()), CompressionFormat::Zlib).is_err());
        assert!(decompress(Cursor::new(gzip), CompressionFormat::Raw).is_err());
    }

    #[test]
    fn garbage_is_rejected_by_every_decompressor() {
        for format in ALL_FORMATS {
            assert!(
                decompress(Cursor::new(vec![0xff; 64]), format).is_err(),
                "{format:?} must reject input that is not a stream"
            );
        }
    }
}
