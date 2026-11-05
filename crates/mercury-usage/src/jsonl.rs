//! Incremental reader for append-only JSONL files that are still being written.

use std::io::{self, Read, Seek, SeekFrom};

/// Remembers how far a JSONL file has been consumed so a file watcher can
/// re-read only what was appended. A trailing line without `\n` is held back
/// until it is complete (the agent may be mid-write).
#[derive(Debug, Clone, Default)]
pub struct JsonlTail {
    offset: u64,
    pending: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TailRead {
    /// Complete, non-empty lines delivered in this call.
    pub lines: usize,
    /// The file was shorter than what was already read (truncated or replaced):
    /// reading restarted from the beginning and callers must drop derived state.
    pub reset: bool,
}

impl JsonlTail {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes read so far, including a held-back partial line.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn read_new<R: Read + Seek>(
        &mut self,
        reader: &mut R,
        mut on_line: impl FnMut(&str),
    ) -> io::Result<TailRead> {
        let len = reader.seek(SeekFrom::End(0))?;
        let mut result = TailRead::default();
        if len < self.offset {
            self.offset = 0;
            self.pending.clear();
            result.reset = true;
        }
        if len == self.offset {
            return Ok(result);
        }
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut appended = Vec::new();
        reader.read_to_end(&mut appended)?;
        self.offset = self.offset.saturating_add(appended.len() as u64);
        self.pending.extend_from_slice(&appended);

        let Some(last_newline) = self.pending.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(result);
        };
        let rest = self.pending.split_off(last_newline + 1);
        let complete = std::mem::replace(&mut self.pending, rest);
        for raw in complete.split(|byte| *byte == b'\n') {
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            if raw.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let line = String::from_utf8_lossy(raw);
            on_line(&line);
            result.lines += 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(tail: &mut JsonlTail, data: &[u8]) -> (Vec<String>, TailRead) {
        let mut lines = Vec::new();
        let result = tail
            .read_new(&mut Cursor::new(data.to_vec()), |line| {
                lines.push(line.to_string())
            })
            .unwrap();
        (lines, result)
    }

    #[test]
    fn holds_back_partial_lines_until_complete() {
        let mut tail = JsonlTail::new();
        let (lines, result) = read(&mut tail, b"{\"a\":1}\n{\"b\":");
        assert_eq!(lines, vec!["{\"a\":1}"]);
        assert_eq!(result.lines, 1);
        assert!(!result.reset);

        let (lines, _) = read(&mut tail, b"{\"a\":1}\n{\"b\":2}\r\n\n{\"c\":3}\n");
        assert_eq!(lines, vec!["{\"b\":2}", "{\"c\":3}"]);

        let (lines, result) = read(&mut tail, b"{\"a\":1}\n{\"b\":2}\r\n\n{\"c\":3}\n");
        assert!(lines.is_empty());
        assert_eq!(result, TailRead::default());
    }

    #[test]
    fn restarts_when_file_shrinks() {
        let mut tail = JsonlTail::new();
        read(&mut tail, b"{\"a\":1}\n{\"b\":2}\n");
        let (lines, result) = read(&mut tail, b"{\"z\":9}\n");
        assert!(result.reset);
        assert_eq!(lines, vec!["{\"z\":9}"]);
        assert_eq!(tail.offset(), 8);
    }
}
