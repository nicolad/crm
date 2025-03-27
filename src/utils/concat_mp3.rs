use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

/// Naive MP3 concatenation:
/// Copies the raw bytes of each input file into the output file in order,
/// without decoding or re-encoding.
///
/// Returns an `std::io::Result<()>` indicating success/failure.
///
/// # Caveat
/// This doesn't validate MP3 frames or remove ID3 tags.
/// The resulting file may have playback issues if each chunk
/// is a fully self-contained MP3 with its own headers.
pub fn concat_mp3(input_files: &[&str], output_file: &str) -> std::io::Result<()> {
    // Create or overwrite the output file
    let mut out = BufWriter::new(File::create(output_file)?);

    for &mp3_path in input_files {
        // Read each chunk as raw bytes
        let mut f = BufReader::new(File::open(mp3_path)?);

        // Copy everything into output
        std::io::copy(&mut f, &mut out)?;
    }

    // Flush ensures all data is written
    out.flush()?;
    Ok(())
}
