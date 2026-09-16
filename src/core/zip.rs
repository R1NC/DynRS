use flate2::{Compression, bufread, write};
use std::io::{self, BufRead, Read, Write};

const BUFFER_SIZE: usize = 16 * 1024; // zlib default chunk size

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
