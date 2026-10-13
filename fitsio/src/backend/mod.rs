/*!
The library that implements the FITS routines

Every call into CFITSIO goes through this module, so the rest of the crate never touches a raw
pointer. Exactly one backend is compiled in:

* [`cfitsio`] (the `cfitsio` feature, on by default) calls the C library through `fitsio-sys`.
  This is the only place in the crate that needs `unsafe` to talk to CFITSIO.
* [`rsfitsio`] (the `rsfitsio` feature) calls the safe (`*_safe`) API of the pure Rust
  [`rsfitsio`](https://crates.io/crates/rsfitsio) port directly. It contains no `unsafe` code.

Both modules expose the same functions, named after their CFITSIO long names, and follow the
CFITSIO conventions: a non-zero `status` on entry makes the call a no-op, and errors are
reported by setting `status`. Buffers are passed as slices, and typed pixel or column data as
bytes together with the CFITSIO datatype code describing them.
*/

#[cfg(all(feature = "cfitsio", feature = "rsfitsio"))]
compile_error!(
    "the `cfitsio` and `rsfitsio` features are mutually exclusive; to use rsfitsio, depend on \
     fitsio with `default-features = false, features = [\"rsfitsio\"]`"
);

#[cfg(not(any(feature = "cfitsio", feature = "rsfitsio")))]
compile_error!("enable one of the `cfitsio` (default) or `rsfitsio` features of fitsio");

#[cfg(feature = "cfitsio")]
pub(crate) mod cfitsio;
#[cfg(feature = "cfitsio")]
pub(crate) use self::cfitsio::*;

#[cfg(all(feature = "rsfitsio", not(feature = "cfitsio")))]
pub(crate) mod rsfitsio;
#[cfg(all(feature = "rsfitsio", not(feature = "cfitsio")))]
pub(crate) use self::rsfitsio::*;

pub(crate) use std::os::raw::{c_char, c_int, c_long};

/// Maximum length of a file name, including the terminating nul (`FLEN_FILENAME`)
pub(crate) const FLEN_FILENAME: usize = 1025;
/// Maximum length of a keyword value string, including the terminating nul (`FLEN_VALUE`)
pub(crate) const FLEN_VALUE: usize = 71;
/// Maximum length of a keyword comment, including the terminating nul (`FLEN_COMMENT`)
pub(crate) const FLEN_COMMENT: usize = 73;
/// Maximum length of an error status message, including the terminating nul (`FLEN_STATUS`)
pub(crate) const FLEN_STATUS: usize = 31;
