//! Version of CFITSIO that `rsfitsio` is a port of

use rsfitsio::fitsio::{CFITSIO_MAJOR, CFITSIO_MINOR};

/// Version of CFITSIO that `rsfitsio` is a port of
pub struct CfitsioVersion {
    /// Minor version
    pub minor: u32,
    /// Major version
    pub major: u32,
}

impl std::fmt::Display for CfitsioVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Return the version of CFITSIO that `rsfitsio` is a port of
pub fn cfitsio_version() -> CfitsioVersion {
    CfitsioVersion {
        minor: CFITSIO_MINOR as u32,
        major: CFITSIO_MAJOR as u32,
    }
}
