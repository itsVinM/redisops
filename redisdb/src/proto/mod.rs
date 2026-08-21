use std::fmt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const SER_NIL: u8 = 0;
pub const SER_ERR: u8 = 1;
pub const SER_STR: u8 = 2;
pub const SER_INT: u8 = 3;
pub const SER_DBL: u8 = 4;
pub const SER_ARR: u8 = 5;

pub const ERR_UNKNOWN: i32 = 1;
pub const ERR_TOO_BIG: i32 = 2;
pub const ERR_TYPE: i32 = 3;
pub const ERR_ARG: i32 = 4;

pub const MAX_MSG_SIZE: u32 = 4 * 1024 * 1024;
pub const MAX_ARGS: u32 = 1024;

#[derive(Debug, Clone)]
pub enum Response {
    Nil,
    Err { code: i32, msg: String },
    Str(String),
    Int(i64),
    Dbl(f64),
    Arr(Vec<Response>),
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Response::Nil => vec![SER_NIL],
            Response::Err { code, msg } => {
                let mut b = Vec::with_capacity(9 + msg.len());
                b.push(SER_ERR);
                b.extend_from_slice(&code.to_le_bytes());
                b.extend_from_slice(&(msg.len() as u32).to_le_bytes());
                b.extend_from_slice(msg.as_bytes());
                b
            }
            Response::Str(s) => {
                let mut b = Vec::with_capacity(5 + s.len());
                b.push(SER_STR);
                b.extend_from_slice(&(s.len() as u32).to_le_bytes());
                b.extend_from_slice(s.as_bytes());
                b
            }
            Response::Int(v) => {
                let mut b = vec![SER_INT];
                b.extend_from_slice(&v.to_le_bytes());
                b
            }
            Response::Dbl(v) => {
                let mut b = vec![SER_DBL];
                b.extend_from_slice(&v.to_le_bytes());
                b
            }
            Response::Arr(elems) => {
                let mut b = vec![SER_ARR];
                b.extend_from_slice(&(elems.len() as u32).to_le_bytes());
                for elem in elems {
                    b.extend_from_slice(&elem.encode());
                }
                b
            }
        }
    }
}

pub async fn read_request<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<String>, ProtoError> {
    let msg_len = r.read_u32_le().await?;
    if msg_len > MAX_MSG_SIZE {
        return Err(ProtoError::TooLarge(msg_len));
    }
    let mut buf = vec![0u8; msg_len as usize];
    r.read_exact(&mut buf).await?;

    if buf.len() < 4 {
        return Err(ProtoError::Malformed("header too short"));
    }
    let n = u32::from_le_bytes(buf[..4].try_into().unwrap());
    if n > MAX_ARGS {
        return Err(ProtoError::TooManyArgs(n));
    }
    let mut buf = &buf[4..];
    let mut args = Vec::with_capacity(n as usize);
    for _ in 0..n {
        if buf.len() < 4 {
            return Err(ProtoError::Malformed("truncated arg header"));
        }
        let sz = u32::from_le_bytes(buf[..4].try_into().unwrap());
        buf = &buf[4..];
        if buf.len() < sz as usize {
            return Err(ProtoError::Malformed("truncated arg body"));
        }
        let arg = String::from_utf8(buf[..sz as usize].to_vec())
            .map_err(|_| ProtoError::Malformed("invalid utf-8"))?;
        args.push(arg);
        buf = &buf[sz as usize..];
    }
    Ok(args)
}

pub async fn write_response<W: AsyncWrite + Unpin>(
    w: &mut W,
    payload: &[u8],
) -> Result<(), ProtoError> {
    let len = payload.len() as u32;
    w.write_u32_le(len).await?;
    w.write_all(payload).await?;
    Ok(())
}

#[derive(Debug)]
pub enum ProtoError {
    Io(std::io::Error),
    TooLarge(u32),
    TooManyArgs(u32),
    Malformed(&'static str),
}

impl From<std::io::Error> for ProtoError {
    fn from(e: std::io::Error) -> Self {
        ProtoError::Io(e)
    }
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Io(e) => write!(f, "io: {}", e),
            ProtoError::TooLarge(n) => write!(f, "message too large: {} bytes", n),
            ProtoError::TooManyArgs(n) => write!(f, "too many arguments: {}", n),
            ProtoError::Malformed(s) => write!(f, "malformed request: {}", s),
        }
    }
}

impl std::error::Error for ProtoError {}
