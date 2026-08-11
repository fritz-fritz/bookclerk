//! Low-level big-endian readers and MP4 box walking.

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Mp4Error, Result};

/// Four-character code.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCC(pub [u8; 4]);

impl FourCC {
    /// Fn.
    pub const fn new(b: &[u8; 4]) -> Self {
        Self(*b)
    }

    /// As str.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl std::fmt::Debug for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Ftyp.
pub const FTYP: FourCC = FourCC::new(b"ftyp");
/// Moov.
pub const MOOV: FourCC = FourCC::new(b"moov");
/// Mdat.
pub const MDAT: FourCC = FourCC::new(b"mdat");
/// Trak.
pub const TRAK: FourCC = FourCC::new(b"trak");
/// Mdia.
pub const MDIA: FourCC = FourCC::new(b"mdia");
/// Minf.
pub const MINF: FourCC = FourCC::new(b"minf");
/// Stbl.
pub const STBL: FourCC = FourCC::new(b"stbl");
/// Stsd.
pub const STSD: FourCC = FourCC::new(b"stsd");
/// Stts.
pub const STTS: FourCC = FourCC::new(b"stts");
/// Stsc.
pub const STSC: FourCC = FourCC::new(b"stsc");
/// Stsz.
pub const STSZ: FourCC = FourCC::new(b"stsz");
/// Stz2.
pub const STZ2: FourCC = FourCC::new(b"stz2");
/// Stco.
pub const STCO: FourCC = FourCC::new(b"stco");
/// Co64.
pub const CO64: FourCC = FourCC::new(b"co64");
/// Mvhd.
pub const MVHD: FourCC = FourCC::new(b"mvhd");
/// Mdhd.
pub const MDHD: FourCC = FourCC::new(b"mdhd");
/// Hdlr.
pub const HDLR: FourCC = FourCC::new(b"hdlr");
/// Aavd.
pub const AAVD: FourCC = FourCC::new(b"aavd");
/// MP4 a.
pub const MP4A: FourCC = FourCC::new(b"mp4a");
/// Enca.
pub const ENCA: FourCC = FourCC::new(b"enca");

// Fragmented (DASH) and Common Encryption boxes. Naming the box types costs
// nothing here and keeps every reader in the workspace spelling them the same
// way; the schemes themselves are a caller's business.
/// Sidx.
pub const SIDX: FourCC = FourCC::new(b"sidx");
/// Moof.
pub const MOOF: FourCC = FourCC::new(b"moof");
/// Traf.
pub const TRAF: FourCC = FourCC::new(b"traf");
/// Tfhd.
pub const TFHD: FourCC = FourCC::new(b"tfhd");
/// Trun.
pub const TRUN: FourCC = FourCC::new(b"trun");
/// Senc.
pub const SENC: FourCC = FourCC::new(b"senc");
/// Sinf.
pub const SINF: FourCC = FourCC::new(b"sinf");
/// Schm.
pub const SCHM: FourCC = FourCC::new(b"schm");
/// Schi.
pub const SCHI: FourCC = FourCC::new(b"schi");
/// Tenc.
pub const TENC: FourCC = FourCC::new(b"tenc");
/// Saiz.
pub const SAIZ: FourCC = FourCC::new(b"saiz");
/// Saio.
pub const SAIO: FourCC = FourCC::new(b"saio");
/// Mvex.
pub const MVEX: FourCC = FourCC::new(b"mvex");
/// Dash.
pub const DASH: FourCC = FourCC::new(b"dash");

/// Header for one ISO-BMFF box.
#[derive(Debug, Clone)]
pub struct BoxHeader {
    /// Start.
    pub start: u64,
    /// Size.
    pub size: u64,
    /// Header len.
    pub header_len: u64,
    /// Kind.
    pub kind: FourCC,
}

impl BoxHeader {
    /// Content start.
    pub fn content_start(&self) -> u64 {
        self.start + self.header_len
    }

    /// Content len.
    pub fn content_len(&self) -> u64 {
        self.size.saturating_sub(self.header_len)
    }

    /// End.
    pub fn end(&self) -> u64 {
        self.start + self.size
    }
}

/// Read u8.
pub fn read_u8(r: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

/// Read u32.
pub fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

/// Read u64.
pub fn read_u64(r: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

/// Read fourcc.
pub fn read_fourcc(r: &mut impl Read) -> Result<FourCC> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(FourCC(buf))
}

/// Read box header.
pub fn read_box_header(r: &mut (impl Read + Seek)) -> Result<BoxHeader> {
    let start = r.stream_position()?;
    let size32 = read_u32(r)?;
    let kind = read_fourcc(r)?;
    let (size, header_len) = if size32 == 1 {
        let size64 = read_u64(r)?;
        (size64, 16u64)
    } else if size32 == 0 {
        // Extends to EOF.
        let end = r.seek(SeekFrom::End(0))?;
        r.seek(SeekFrom::Start(start + 8))?;
        (end - start, 8u64)
    } else {
        (u64::from(size32), 8u64)
    };
    if size < header_len {
        return Err(Mp4Error::container(format!(
            "box {kind} at {start} has size {size} < header {header_len}"
        )));
    }
    Ok(BoxHeader {
        start,
        size,
        header_len,
        kind,
    })
}

/// Iterate immediate children of a container box whose content starts at `start`
/// and ends at `end`.
///
/// The visitor keeps its own error type, so a caller reading boxes this crate
/// knows nothing about can fail in its own vocabulary.
pub fn walk_children<R, F, E>(
    r: &mut R,
    start: u64,
    end: u64,
    mut visit: F,
) -> std::result::Result<(), E>
where
    R: Read + Seek,
    F: FnMut(&mut R, &BoxHeader) -> std::result::Result<(), E>,
    E: From<Mp4Error>,
{
    let seek = |r: &mut R, to: u64| -> std::result::Result<u64, E> {
        r.seek(SeekFrom::Start(to))
            .map_err(|err| E::from(Mp4Error::from(err)))
    };
    seek(r, start)?;
    loop {
        let pos = r
            .stream_position()
            .map_err(|err| E::from(Mp4Error::from(err)))?;
        if pos + 8 > end {
            break;
        }
        let header = read_box_header(r).map_err(E::from)?;
        if header.end() > end {
            return Err(E::from(Mp4Error::container(format!(
                "box {} overflows parent (end {} > {end})",
                header.kind,
                header.end()
            ))));
        }
        let next = header.end();
        visit(r, &header)?;
        seek(r, next)?;
    }
    Ok(())
}

/// Find child.
pub fn find_child<R: Read + Seek>(
    r: &mut R,
    start: u64,
    end: u64,
    want: FourCC,
) -> Result<Option<BoxHeader>> {
    let mut found = None;
    walk_children(r, start, end, |_, header| -> Result<()> {
        if header.kind == want && found.is_none() {
            found = Some(header.clone());
        }
        Ok(())
    })?;
    Ok(found)
}

/// Read full box version flags.
pub fn read_full_box_version_flags(r: &mut impl Read) -> Result<(u8, u32)> {
    let version = read_u8(r)?;
    let mut flags = [0u8; 3];
    r.read_exact(&mut flags)?;
    let flags_u32 = (u32::from(flags[0]) << 16) | (u32::from(flags[1]) << 8) | u32::from(flags[2]);
    Ok((version, flags_u32))
}
