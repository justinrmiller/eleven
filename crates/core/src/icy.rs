//! Shoutcast/Icecast in-band metadata ("ICY") demultiplexing.
//!
//! When a client sends `Icy-MetaData: 1`, the server answers with an `icy-metaint: N` header and
//! inserts a metadata block after every `N` bytes of audio. Each block starts with one length
//! byte (`len * 16` bytes follow), padded with NULs, containing text like
//! `StreamTitle='Artist - Song';StreamUrl='';`.

/// Splits an ICY stream into audio bytes and metadata blocks.
#[derive(Debug, Clone)]
pub struct IcyDemuxer {
    metaint: usize,
    state: State,
}

#[derive(Debug, Clone)]
enum State {
    /// Copying audio; `remaining` bytes until the next metadata length byte.
    Audio { remaining: usize },
    /// The next byte is a metadata length.
    Length,
    /// Collecting a metadata block.
    Meta { remaining: usize, buf: Vec<u8> },
}

impl IcyDemuxer {
    /// Creates a demuxer for the given `icy-metaint`. `0` means "no metadata": all bytes are
    /// passed through as audio.
    #[must_use]
    pub fn new(metaint: usize) -> Self {
        Self {
            metaint,
            state: State::Audio { remaining: metaint },
        }
    }

    /// Consumes `input`, appending audio bytes to `audio` and returning any complete metadata
    /// blocks (decoded to text, NUL padding removed). Empty blocks are skipped.
    pub fn feed(&mut self, mut input: &[u8], audio: &mut Vec<u8>) -> Vec<String> {
        if self.metaint == 0 {
            audio.extend_from_slice(input);
            return Vec::new();
        }
        let mut blocks = Vec::new();
        while !input.is_empty() {
            match &mut self.state {
                State::Audio { remaining } => {
                    let n = (*remaining).min(input.len());
                    audio.extend_from_slice(&input[..n]);
                    input = &input[n..];
                    *remaining -= n;
                    if *remaining == 0 {
                        self.state = State::Length;
                    }
                }
                State::Length => {
                    let len = usize::from(input[0]) * 16;
                    input = &input[1..];
                    self.state = if len == 0 {
                        State::Audio {
                            remaining: self.metaint,
                        }
                    } else {
                        State::Meta {
                            remaining: len,
                            buf: Vec::with_capacity(len),
                        }
                    };
                }
                State::Meta { remaining, buf } => {
                    let n = (*remaining).min(input.len());
                    buf.extend_from_slice(&input[..n]);
                    input = &input[n..];
                    *remaining -= n;
                    if *remaining == 0 {
                        let text = decode_metadata(buf);
                        if !text.is_empty() {
                            blocks.push(text);
                        }
                        self.state = State::Audio {
                            remaining: self.metaint,
                        };
                    }
                }
            }
        }
        blocks
    }
}

/// Decodes a metadata block: strips NUL padding, then decodes UTF-8, falling back to Latin-1.
#[must_use]
pub fn decode_metadata(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    let bytes = &bytes[..end];
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// Extracts the `StreamTitle` value from a metadata block, if present and non-empty.
///
/// Titles may themselves contain apostrophes (`Don't Stop`), so the value ends at the first
/// `';` sequence rather than the first quote.
#[must_use]
pub fn parse_stream_title(meta: &str) -> Option<String> {
    const KEY: &str = "StreamTitle='";
    let start = meta.find(KEY)? + KEY.len();
    let rest = &meta[start..];
    let value = match rest.find("';") {
        Some(end) => &rest[..end],
        None => rest.strip_suffix('\'').unwrap_or(rest),
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Encodes a metadata block exactly as a server would (used by tests and the test server).
#[must_use]
pub fn encode_metadata(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let blocks = bytes.len().div_ceil(16).min(255);
    let mut out = Vec::with_capacity(1 + blocks * 16);
    // `blocks` is clamped to 255, so it fits in a byte.
    #[allow(clippy::cast_possible_truncation)]
    out.push(blocks as u8);
    out.extend_from_slice(&bytes[..bytes.len().min(blocks * 16)]);
    out.resize(1 + blocks * 16, 0);
    out
}

/// Interleaves `audio` with metadata blocks every `metaint` bytes, as a server would.
/// `meta_at(i)` supplies the text for the `i`-th block (`None` = empty block).
#[must_use]
pub fn interleave(
    audio: &[u8],
    metaint: usize,
    mut meta_at: impl FnMut(usize) -> Option<String>,
) -> Vec<u8> {
    if metaint == 0 {
        return audio.to_vec();
    }
    let mut out = Vec::with_capacity(audio.len() + audio.len() / metaint + 64);
    for (i, chunk) in audio.chunks(metaint).enumerate() {
        out.extend_from_slice(chunk);
        if chunk.len() == metaint {
            match meta_at(i) {
                Some(text) => out.extend(encode_metadata(&text)),
                None => out.push(0),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn passthrough_without_metaint() {
        let mut d = IcyDemuxer::new(0);
        let mut audio = Vec::new();
        assert!(d.feed(b"abc\x01def", &mut audio).is_empty());
        assert_eq!(audio, b"abc\x01def");
    }

    #[test]
    fn splits_audio_and_metadata() {
        let stream = interleave(b"0123456789AB", 4, |i| {
            (i == 0).then(|| "StreamTitle='Hello';".to_owned())
        });
        let mut d = IcyDemuxer::new(4);
        let mut audio = Vec::new();
        let blocks = d.feed(&stream, &mut audio);
        assert_eq!(audio, b"0123456789AB");
        assert_eq!(blocks, ["StreamTitle='Hello';"]);
    }

    #[test]
    fn byte_at_a_time_feeding() {
        let stream = interleave(&[7u8; 40], 8, |i| Some(format!("StreamTitle='T{i}';")));
        let mut d = IcyDemuxer::new(8);
        let mut audio = Vec::new();
        let mut blocks = Vec::new();
        for b in &stream {
            blocks.extend(d.feed(std::slice::from_ref(b), &mut audio));
        }
        assert_eq!(audio, [7u8; 40]);
        assert_eq!(blocks.len(), 5);
        assert_eq!(parse_stream_title(&blocks[4]).as_deref(), Some("T4"));
    }

    #[test]
    fn stream_titles() {
        assert_eq!(
            parse_stream_title(
                "StreamTitle='Spinal Tap - Tonight I'm Gonna Rock You';StreamUrl='';"
            )
            .as_deref(),
            Some("Spinal Tap - Tonight I'm Gonna Rock You")
        );
        assert_eq!(
            parse_stream_title("StreamTitle='No terminator'").as_deref(),
            Some("No terminator")
        );
        assert_eq!(parse_stream_title("StreamTitle='';"), None);
        assert_eq!(parse_stream_title("StreamUrl='x';"), None);
        assert_eq!(
            parse_stream_title("StreamTitle='  padded ';").as_deref(),
            Some("padded")
        );
    }

    #[test]
    fn metadata_decoding() {
        assert_eq!(decode_metadata(b"abc\0\0\0"), "abc");
        assert_eq!(decode_metadata(b"\0\0"), "");
        assert_eq!(decode_metadata("caf\u{e9}".as_bytes()), "caf\u{e9}");
        assert_eq!(decode_metadata(b"caf\xe9"), "caf\u{e9}", "Latin-1 fallback");
    }

    #[test]
    fn encoding_pads_and_truncates() {
        assert_eq!(encode_metadata(""), [0]);
        let e = encode_metadata("abc");
        assert_eq!(e.len(), 17);
        assert_eq!(e[0], 1);
        let long = "x".repeat(5000);
        let e = encode_metadata(&long);
        assert_eq!(e[0], 255);
        assert_eq!(e.len(), 1 + 255 * 16);
    }

    #[test]
    fn interleave_without_metaint_is_identity() {
        assert_eq!(interleave(b"abc", 0, |_| None), b"abc");
    }

    proptest! {
        #[test]
        fn demux_recovers_audio_for_any_chunking(
            audio in proptest::collection::vec(any::<u8>(), 0..2000),
            metaint in 1usize..300,
            titles in proptest::collection::vec(proptest::option::of("[a-zA-Z0-9 ']{1,40}"), 0..20),
            chunks in proptest::collection::vec(1usize..97, 1..50),
        ) {
            let stream = interleave(&audio, metaint, |i| {
                titles.get(i).cloned().flatten().map(|t| format!("StreamTitle='{t}';"))
            });
            let mut d = IcyDemuxer::new(metaint);
            let mut out = Vec::new();
            let mut blocks = Vec::new();
            let mut rest = stream.as_slice();
            let mut k = 0;
            while !rest.is_empty() {
                let n = chunks[k % chunks.len()].min(rest.len());
                blocks.extend(d.feed(&rest[..n], &mut out));
                rest = &rest[n..];
                k += 1;
            }
            prop_assert_eq!(&out, &audio);
            let full_chunks = audio.len() / metaint;
            let expected: Vec<String> = (0..full_chunks)
                .filter_map(|i| titles.get(i).cloned().flatten())
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect();
            let got: Vec<String> = blocks.iter().filter_map(|b| parse_stream_title(b)).collect();
            prop_assert_eq!(got, expected);
        }
    }
}
