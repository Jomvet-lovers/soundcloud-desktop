use tokio::io::{AsyncWrite, AsyncWriteExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Strategy {
    None = 0,
    TlsRec = 1,
    Split = 2,
    TlsRecSplit = 3,
    MultiSplit = 4,
}

pub const LADDER: [Strategy; 5] = [
    Strategy::None,
    Strategy::TlsRec,
    Strategy::Split,
    Strategy::TlsRecSplit,
    Strategy::MultiSplit,
];

struct Tls<'a> {
    payload: &'a [u8],
    sni: Option<(usize, usize)>,
}

fn parse_tls_clienthello(buf: &[u8]) -> Option<Tls<'_>> {
    if buf.len() < 5 || buf[0] != 0x16 || buf[1] != 0x03 {
        return None;
    }
    let rec_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    let end = 5 + rec_len;
    if buf.len() < end {
        return None;
    }
    let p = &buf[5..end];
    if p.is_empty() || p[0] != 0x01 {
        return None;
    }
    let mut i = 4 + 2 + 32;
    if p.len() < i + 1 {
        return None;
    }
    i += 1 + p[i] as usize;
    if p.len() < i + 2 {
        return None;
    }
    i += 2 + u16::from_be_bytes([p[i], p[i + 1]]) as usize;
    if p.len() < i + 1 {
        return None;
    }
    i += 1 + p[i] as usize;
    if p.len() < i + 2 {
        return None;
    }
    let ext_total = u16::from_be_bytes([p[i], p[i + 1]]) as usize;
    i += 2;
    let ext_end = (i + ext_total).min(p.len());
    while i + 4 <= ext_end {
        let etype = u16::from_be_bytes([p[i], p[i + 1]]);
        let elen = u16::from_be_bytes([p[i + 2], p[i + 3]]) as usize;
        let body = i + 4;
        if etype == 0x0000 && body + 5 <= p.len() {
            let name_len = u16::from_be_bytes([p[body + 3], p[body + 4]]) as usize;
            let host_start = body + 5;
            if host_start + name_len <= p.len() {
                return Some(Tls {
                    payload: p,
                    sni: Some((5 + host_start, name_len)),
                });
            }
        }
        i = body + elen;
    }
    Some(Tls {
        payload: p,
        sni: None,
    })
}

fn split_points(buf: &[u8]) -> (usize, Vec<usize>) {
    match parse_tls_clienthello(buf) {
        Some(t) => {
            let mid = match t.sni {
                Some((off, len)) => off + len / 2,
                None => 5 + t.payload.len() / 2,
            };
            let mid = mid.clamp(6, buf.len().saturating_sub(1).max(6));
            let multi: Vec<usize> = [1usize, 5 + 1, mid, mid + (buf.len() - mid) / 2]
                .into_iter()
                .filter(|&p| p > 0 && p < buf.len())
                .collect();
            (mid, dedup_sorted(multi))
        }
        None => {
            let mid = (buf.len() / 2).max(1).min(buf.len().saturating_sub(1).max(1));
            (mid, vec![mid])
        }
    }
}

fn dedup_sorted(mut v: Vec<usize>) -> Vec<usize> {
    v.sort_unstable();
    v.dedup();
    v
}

async fn write_seg<W: AsyncWrite + Unpin>(w: &mut W, b: &[u8]) -> std::io::Result<()> {
    w.write_all(b).await?;
    w.flush().await
}

async fn write_tlsrec<W: AsyncWrite + Unpin>(
    w: &mut W,
    buf: &[u8],
    flush_between: bool,
) -> std::io::Result<()> {
    let t = match parse_tls_clienthello(buf) {
        Some(t) if t.payload.len() > 16 => t,
        _ => return write_seg(w, buf).await,
    };
    let a = match t.sni {
        Some((off, len)) => (off - 5 + len / 2).clamp(1, t.payload.len() - 1),
        None => t.payload.len() / 2,
    };
    let mk = |s: &[u8]| {
        let mut r = Vec::with_capacity(s.len() + 5);
        r.push(0x16);
        r.push(buf[1]);
        r.push(buf[2]);
        r.extend_from_slice(&(s.len() as u16).to_be_bytes());
        r.extend_from_slice(s);
        r
    };
    let r1 = mk(&t.payload[..a]);
    let r2 = mk(&t.payload[a..]);
    let tail = &buf[5 + t.payload.len()..];
    if flush_between {
        write_seg(w, &r1).await?;
        write_seg(w, &r2).await?;
    } else {
        let mut o = r1;
        o.extend_from_slice(&r2);
        w.write_all(&o).await?;
        w.flush().await?;
    }
    if !tail.is_empty() {
        write_seg(w, tail).await?;
    }
    Ok(())
}

pub async fn apply<W: AsyncWrite + Unpin>(
    w: &mut W,
    buf: &[u8],
    strategy: Strategy,
) -> std::io::Result<()> {
    match strategy {
        Strategy::None => write_seg(w, buf).await,
        Strategy::TlsRec => write_tlsrec(w, buf, false).await,
        Strategy::TlsRecSplit => write_tlsrec(w, buf, true).await,
        Strategy::Split => {
            let (pos, _) = split_points(buf);
            write_seg(w, &buf[..pos]).await?;
            write_seg(w, &buf[pos..]).await
        }
        Strategy::MultiSplit => {
            let (_, points) = split_points(buf);
            let mut prev = 0;
            for p in points {
                if p > prev {
                    write_seg(w, &buf[prev..p]).await?;
                    prev = p;
                }
            }
            if prev < buf.len() {
                write_seg(w, &buf[prev..]).await?;
            }
            Ok(())
        }
    }
}
