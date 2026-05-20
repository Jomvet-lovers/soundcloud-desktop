use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpListener, TcpStream};
use tokio::time::timeout;
use tracing::{debug, trace};

use crate::state::State;
use crate::tamper::{apply, Strategy};

const FIRST_FLIGHT_WAIT: Duration = Duration::from_millis(150);
const FIRST_FLIGHT_MAX: usize = 16 * 1024;

pub async fn run(listener: TcpListener, state: Arc<State>) {
    loop {
        let (sock, _) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                debug!(?e, "socks accept");
                continue;
            }
        };
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(sock, st).await {
                trace!(?e, "socks session");
            }
        });
    }
}

async fn handle(mut c: TcpStream, state: Arc<State>) -> std::io::Result<()> {
    c.set_nodelay(true)?;

    let mut hdr = [0u8; 2];
    c.read_exact(&mut hdr).await?;
    if hdr[0] != 0x05 {
        return Err(std::io::Error::other("not socks5"));
    }
    let mut methods = vec![0u8; hdr[1] as usize];
    c.read_exact(&mut methods).await?;
    c.write_all(&[0x05, 0x00]).await?;

    let mut req = [0u8; 4];
    c.read_exact(&mut req).await?;
    if req[1] != 0x01 {
        c.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Err(std::io::Error::other("cmd unsupported"));
    }
    let host: String = match req[3] {
        0x01 => {
            let mut a = [0u8; 4];
            c.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        0x03 => {
            let mut l = [0u8; 1];
            c.read_exact(&mut l).await?;
            let mut s = vec![0u8; l[0] as usize];
            c.read_exact(&mut s).await?;
            String::from_utf8_lossy(&s).into_owned()
        }
        0x04 => {
            let mut a = [0u8; 16];
            c.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        _ => return Err(std::io::Error::other("atyp unsupported")),
    };
    let mut p = [0u8; 2];
    c.read_exact(&mut p).await?;
    let port = u16::from_be_bytes(p);

    let addr = match resolve(&host, port).await {
        Ok(a) => a,
        Err(_) => {
            c.write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await?;
            return Ok(());
        }
    };
    let mut up = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(_) => {
            c.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await?;
            return Ok(());
        }
    };
    up.set_nodelay(true)?;
    set_mss(&up, 88);

    c.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;

    let effective = if state.is_enabled() {
        state.strategy()
    } else {
        Strategy::None
    };

    if matches!(effective, Strategy::None) {
        tokio::io::copy_bidirectional(&mut c, &mut up).await?;
        return Ok(());
    }

    let mut first = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    loop {
        match timeout(FIRST_FLIGHT_WAIT, c.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                first.extend_from_slice(&tmp[..n]);
                if first.len() >= FIRST_FLIGHT_MAX || record_complete(&first) {
                    break;
                }
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => break,
        }
    }

    if !first.is_empty() {
        apply(&mut up, &first, effective).await?;
    }
    tokio::io::copy_bidirectional(&mut c, &mut up).await?;
    Ok(())
}

fn record_complete(b: &[u8]) -> bool {
    if b.len() < 5 || b[0] != 0x16 {
        return false;
    }
    let l = u16::from_be_bytes([b[3], b[4]]) as usize;
    b.len() >= 5 + l
}

#[cfg(unix)]
fn set_mss(s: &TcpStream, mss: u32) {
    use std::os::fd::AsRawFd;
    let fd = s.as_raw_fd();
    let v: libc::c_int = mss as libc::c_int;
    unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_MAXSEG,
            &v as *const _ as *const _,
            std::mem::size_of_val(&v) as libc::socklen_t,
        );
    }
}

#[cfg(not(unix))]
fn set_mss(_s: &TcpStream, _mss: u32) {}

async fn resolve(host: &str, port: u16) -> std::io::Result<SocketAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    lookup_host((host, port))
        .await?
        .next()
        .ok_or_else(|| std::io::Error::other("no addrs"))
}
