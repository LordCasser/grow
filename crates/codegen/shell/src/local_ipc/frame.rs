use std::io;

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME_SIZE: u32 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("local IPC connection closed")]
    ConnectionClosed,
    #[error("local IPC frame is {actual} bytes; maximum is {MAX_FRAME_SIZE}")]
    TooLarge { actual: u64 },
    #[error("local IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("local IPC JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

pub async fn read_json<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::ConnectionClosed);
        }
        Err(error) => return Err(FrameError::Io(error)),
    }

    let length = u32::from_be_bytes(length);
    if length > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge {
            actual: u64::from(length),
        });
    }

    let mut payload = vec![0_u8; length as usize];
    reader.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

pub async fn write_json<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize + ?Sized,
{
    let payload = serde_json::to_vec(value)?;
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge {
        actual: payload.len() as u64,
    })?;
    if length > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge {
            actual: u64::from(length),
        });
    }

    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Message {
        value: String,
    }

    #[tokio::test]
    async fn json_round_trip_uses_big_endian_length_prefix() {
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let message = Message {
            value: "hello".to_owned(),
        };

        write_json(&mut writer, &message).await.unwrap();
        let decoded: Message = read_json(&mut reader).await.unwrap();

        assert_eq!(decoded, message);
    }

    #[tokio::test]
    async fn oversized_inbound_frame_is_rejected_before_payload_read() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer
            .write_all(&(MAX_FRAME_SIZE + 1).to_be_bytes())
            .await
            .unwrap();

        let error = read_json::<_, serde_json::Value>(&mut reader)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            FrameError::TooLarge { actual } if actual == u64::from(MAX_FRAME_SIZE + 1)
        ));
    }

    #[tokio::test]
    async fn oversized_outbound_frame_is_rejected() {
        let (mut writer, _reader) = tokio::io::duplex(8);
        let message = Message {
            value: "x".repeat(MAX_FRAME_SIZE as usize),
        };

        let error = write_json(&mut writer, &message).await.unwrap_err();

        assert!(matches!(error, FrameError::TooLarge { .. }));
    }
}
