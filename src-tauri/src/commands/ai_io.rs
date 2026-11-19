//! Bounded CLI pipe readers. Drain stderr concurrently so a full pipe cannot
//! block stdout; keep only a small diagnostic prefix.
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt};

pub(super) const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;

pub(super) async fn next_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let read = reader
        .take((MAX_LINE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if read == 0 {
        return Ok(None);
    }
    if read > MAX_LINE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "AI output record exceeded the 2 MiB reading limit",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub(super) async fn drain_stderr<R: AsyncRead + Unpin>(mut reader: R) -> std::io::Result<String> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_STDERR_BYTES - kept.len();
        let used = read.min(remaining);
        kept.extend_from_slice(&chunk[..used]);
        truncated |= used < read;
    }
    let mut text = String::from_utf8_lossy(&kept).into_owned();
    if truncated {
        text.push_str("\n[CLI diagnostic shortened]");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn rejects_newline_free_output_at_the_bound() {
        let mut reader = BufReader::new(std::io::Cursor::new(vec![b'x'; MAX_LINE_BYTES * 3]));
        assert_eq!(
            next_line(&mut reader).await.unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        let mut reader = BufReader::new(std::io::Cursor::new("한\nnext".as_bytes()));
        assert_eq!(
            next_line(&mut reader).await.unwrap().as_deref(),
            Some("한\n")
        );
        assert_eq!(
            next_line(&mut reader).await.unwrap().as_deref(),
            Some("next")
        );
        assert!(next_line(&mut reader).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn drains_diagnostics_beyond_the_pipe_capacity_but_bounds_retained_text() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let write = tokio::spawn(async move {
            for _ in 0..256 {
                writer.write_all(&[b'e'; 4096]).await.unwrap();
            }
        });
        let diagnostic =
            tokio::time::timeout(std::time::Duration::from_secs(5), drain_stderr(reader))
                .await
                .unwrap()
                .unwrap();
        write.await.unwrap();
        assert!(diagnostic.len() < MAX_STDERR_BYTES + 100);
        assert!(diagnostic.ends_with("[CLI diagnostic shortened]"));
    }
}
