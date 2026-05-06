use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::io::{AsyncRead, AsyncReadExt};

/// PROXY v1 line: `PROXY <TCP4|TCP6|UNKNOWN> <src> <dst> <sport> <dport>\r\n`
/// Spec: max 107 байт payload + CRLF = 109. Берём 256 с запасом.
pub(crate) async fn read_proxy_v1<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<SocketAddr> {
    let mut buf = [0u8; 256];
    let mut pos = 0;
    loop {
        if pos >= buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "PROXY v1 line too long"));
        }
        let n = stream.read(&mut buf[pos..pos + 1]).await?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF before PROXY v1 CRLF"));
        }
        pos += 1;
        if pos >= 2 && &buf[pos - 2..pos] == b"\r\n" {
            break;
        }
    }
    let line = std::str::from_utf8(&buf[..pos - 2])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "PROXY v1 not utf8"))?;

    let mut parts = line.split_ascii_whitespace();
    if parts.next() != Some("PROXY") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing PROXY signature"));
    }
    let proto = parts.next().unwrap_or("");
    if proto == "UNKNOWN" {
        return Err(io::Error::new(io::ErrorKind::Other, "PROXY v1 UNKNOWN proto"));
    }
    let src_ip = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PROXY v1 missing src_ip"))?;
    let _ = parts.next(); // dst_ip
    let src_port = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PROXY v1 missing src_port"))?;

    let ip: IpAddr = src_ip
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad src_ip"))?;
    let port: u16 = src_port
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad src_port"))?;
    Ok(SocketAddr::new(ip, port))
}
