use unicode_segmentation::UnicodeSegmentation;

/// Splits `text` into chunks of up to `max_chars` *graphemes* each,
/// ensuring you never break a Unicode character in half.
pub fn chunk_text_unicode(text: &str, max_chars: usize) -> Vec<String> {
    let mut results = Vec::new();
    let mut current_chunk = String::with_capacity(max_chars);
    let mut current_count = 0;

    // Iterate over graphemes (what we usually call "characters")
    for grapheme in text.graphemes(true) {
        // If adding another grapheme would exceed `max_chars`,
        // push the current chunk and start a fresh one.
        if current_count >= max_chars {
            results.push(current_chunk);
            current_chunk = String::with_capacity(max_chars);
            current_count = 0;
        }

        current_chunk.push_str(grapheme);
        current_count += 1;
    }

    // Push the final chunk if there's any leftover
    if !current_chunk.is_empty() {
        results.push(current_chunk);
    }

    results
}
