/*!
The library that implements the FITS routines

Every call into CFITSIO goes through this module, so the rest of the crate never touches a raw
pointer. Exactly one backend is compiled in:

* [`cfitsio`] (the `cfitsio` feature, on by default) calls the C library through `fitsio-sys`.
  This is the only place in the crate that needs `unsafe` to talk to CFITSIO.
* [`rsfitsio`] (the experimental `rsfitsio-experimental` feature) calls the safe (`*_safe`) API
  of the pure Rust [`rsfitsio`](https://crates.io/crates/rsfitsio) port directly. It contains no
  `unsafe` code.

Both modules expose the same functions, named after their CFITSIO long names, and follow the
CFITSIO conventions: a non-zero `status` on entry makes the call a no-op, and errors are
reported by setting `status`. Buffers are passed as slices, and typed pixel or column data as
bytes together with the CFITSIO datatype code describing them.
*/

#[cfg(all(feature = "cfitsio", feature = "rsfitsio-experimental"))]
compile_error!(
    "the `cfitsio` and `rsfitsio-experimental` features are mutually exclusive; to use rsfitsio, depend on \
     fitsio with `default-features = false, features = [\"rsfitsio-experimental\"]`"
);

#[cfg(not(any(feature = "cfitsio", feature = "rsfitsio-experimental")))]
compile_error!(
    "enable one of the `cfitsio` (default) or `rsfitsio-experimental` features of fitsio"
);

#[cfg(feature = "cfitsio")]
pub(crate) mod cfitsio;
#[cfg(feature = "cfitsio")]
pub(crate) use self::cfitsio::*;

#[cfg(all(feature = "rsfitsio-experimental", not(feature = "cfitsio")))]
pub(crate) mod rsfitsio;
#[cfg(all(feature = "rsfitsio-experimental", not(feature = "cfitsio")))]
pub(crate) use self::rsfitsio::*;

use std::convert::TryFrom;
use std::ffi::CString;
pub(crate) use std::os::raw::{c_char, c_int, c_long};

/// Maximum length of a file name, including the terminating nul (`FLEN_FILENAME`)
pub(crate) const FLEN_FILENAME: usize = 1025;
/// Maximum length of a keyword value string, including the terminating nul (`FLEN_VALUE`)
pub(crate) const FLEN_VALUE: usize = 71;
/// Maximum length of a keyword comment, including the terminating nul (`FLEN_COMMENT`)
pub(crate) const FLEN_COMMENT: usize = 73;
/// Maximum length of an error status message, including the terminating nul (`FLEN_STATUS`)
pub(crate) const FLEN_STATUS: usize = 31;

/// CFITSIO status for an array with the wrong number of dimensions (`BAD_DIMEN`)
pub(crate) const BAD_DIMEN: c_int = 320;
/// CFITSIO status for a first pixel number after the last (`BAD_PIX_NUM`)
pub(crate) const BAD_PIX_NUM: c_int = 321;
/// CFITSIO status for an illegal TFORM format code (`BAD_TFORM`)
pub(crate) const BAD_TFORM: c_int = 261;
/// CFITSIO status for a missing keyword (`KEY_NO_EXIST`)
const KEY_NO_EXIST: c_int = 202;
/// CFITSIO datatype code of string columns (`TSTRING`)
const TSTRING: c_int = 16;
/// Longest `TDISPn` value CFITSIO can hold: it reads them into 20 character buffers
const MAX_TDISP_LEN: usize = 19;
/// Widest number CFITSIO can format as a string: it formats into 400 character buffers
const MAX_NUMBER_WIDTH: c_int = 399;

/// Number of pixels in a region of the current image, or `None` with `status` set if the region
/// is not valid for it.
///
/// CFITSIO reads one entry per axis from each of the arrays, so a region with too few entries
/// would make it read past their ends.
pub(crate) fn checked_subset_len(
    f: &mut Handle,
    fpixel: &[c_long],
    lpixel: &[c_long],
    inc: Option<&[c_long]>,
    status: &mut c_int,
) -> Option<usize> {
    let mut naxis = 0;
    fits_get_img_dim(f, &mut naxis, status);
    if *status != 0 {
        return None;
    }
    match subset_len(naxis, fpixel, lpixel, inc) {
        Ok(len) => Some(len),
        Err(e) => {
            *status = e;
            None
        }
    }
}

/// Number of pixels in the region running from the 1-based `fpixel` to `lpixel` inclusive,
/// stepping by `inc` (or 1), of an image with `naxis` axes, or a CFITSIO status if the region
/// does not have one entry per axis.
///
/// The region is not checked against the image's size: like CFITSIO, a range past the end of an
/// axis carries on into the following data.
fn subset_len(
    naxis: c_int,
    fpixel: &[c_long],
    lpixel: &[c_long],
    inc: Option<&[c_long]>,
) -> Result<usize, c_int> {
    let naxis = usize::try_from(naxis).map_err(|_| BAD_DIMEN)?;
    if fpixel.len() != naxis || lpixel.len() != naxis || inc.map_or(false, |i| i.len() != naxis) {
        return Err(BAD_DIMEN);
    }

    let mut len: usize = 1;
    for axis in 0..naxis {
        let (first, last) = (fpixel[axis], lpixel[axis]);
        let step = inc.map_or(1, |i| i[axis]);
        if first < 1 || last < first || step < 1 {
            return Err(BAD_PIX_NUM);
        }
        let n = usize::try_from((last - first) / step + 1).map_err(|_| BAD_PIX_NUM)?;
        len = len.checked_mul(n).ok_or(BAD_PIX_NUM)?;
    }
    Ok(len)
}

/// Size of each buffer that reading column `colnum` as strings needs, including the terminating
/// nul, or `None` with `status` set if the column cannot be read safely this way.
pub(crate) fn col_str_buffer_len(
    f: &mut Handle,
    colnum: c_int,
    status: &mut c_int,
) -> Option<usize> {
    // CFITSIO copies `TDISPn` into a fixed size buffer, in `fits_get_col_display_width` among
    // others, so check its length before calling those
    let keyname = CString::new(format!("TDISP{}", colnum)).expect("no nul in keyword name");
    let mut tdisp = [0; FLEN_VALUE];
    let mut comment = [0; FLEN_COMMENT];
    let mut tstatus = 0;
    fits_read_key_str(f, &keyname, &mut tdisp, &mut comment, &mut tstatus);
    let tdisp: &[c_char] = match tstatus {
        0 => &tdisp[..tdisp.iter().position(|c| *c == 0).unwrap_or(tdisp.len())],
        KEY_NO_EXIST => &[],
        e => {
            *status = e;
            return None;
        }
    };
    // CFITSIO parses the width without checking for overflow, and rsfitsio can take minutes
    // over a huge one, so refuse those before either sees them
    if tdisp.len() > MAX_TDISP_LEN || !tdisp_width_ok(tdisp) {
        *status = BAD_TFORM;
        return None;
    }

    let mut display_width = 0;
    let (mut typecode, mut repeat, mut width) = (0, 0, 0);
    fits_get_col_display_width(f, colnum, &mut display_width, status);
    fits_get_coltype(f, colnum, &mut typecode, &mut repeat, &mut width, status);
    if *status != 0 {
        return None;
    }
    match col_str_len(display_width, typecode, repeat, width) {
        Ok(len) => Some(len),
        Err(e) => {
            *status = e;
            None
        }
    }
}

/// Whether the field width of a `TDISPn` display format, such as the `12` of `F12.4`, is at most
/// the widest number CFITSIO can format
fn tdisp_width_ok(tdisp: &[c_char]) -> bool {
    let digits = tdisp
        .iter()
        .map(|c| *c as u8)
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit());
    let mut width: c_int = 0;
    for digit in digits {
        width = width * 10 + c_int::from(digit - b'0');
        if width > MAX_NUMBER_WIDTH {
            return false;
        }
    }
    true
}

/// Size of each string buffer for a column, from its display width, type code, repeat count and
/// width, or a CFITSIO status if it cannot be read safely as strings.
///
/// CFITSIO copies a string column's full width, ignoring its display width, and formats other
/// columns in their display width, or 20 characters for 64-bit integers. Variable-length string
/// columns are refused: their width varies per row.
fn col_str_len(
    display_width: c_int,
    typecode: c_int,
    repeat: i64,
    width: i64,
) -> Result<usize, c_int> {
    if typecode == -TSTRING {
        return Err(BAD_TFORM);
    }
    let len = if typecode == TSTRING {
        repeat.max(width)
    } else {
        if !(1..=MAX_NUMBER_WIDTH).contains(&display_width) {
            return Err(BAD_TFORM);
        }
        i64::from(display_width).max(20)
    };
    usize::try_from(len.max(0) + 1).map_err(|_| BAD_TFORM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_len_counts_pixels() {
        assert_eq!(subset_len(2, &[1, 1], &[3, 2], None), Ok(6));
        assert_eq!(subset_len(2, &[1, 1], &[5, 2], Some(&[2, 1])), Ok(6));
    }

    #[test]
    fn subset_len_needs_one_entry_per_axis() {
        assert_eq!(subset_len(2, &[1], &[3], None), Err(BAD_DIMEN));
        assert_eq!(subset_len(2, &[1, 1], &[3, 2], Some(&[1])), Err(BAD_DIMEN));
    }

    #[test]
    fn subset_len_refuses_backwards_regions() {
        assert_eq!(subset_len(1, &[3], &[2], None), Err(BAD_PIX_NUM));
        assert_eq!(subset_len(1, &[0], &[2], None), Err(BAD_PIX_NUM));
        assert_eq!(subset_len(1, &[1], &[2], Some(&[0])), Err(BAD_PIX_NUM));
    }

    #[test]
    fn col_str_len_covers_the_string_width() {
        // a `20A` column, whatever its display width
        assert_eq!(col_str_len(5, TSTRING, 20, 20), Ok(21));
        assert_eq!(col_str_len(1, TSTRING, 1, 1), Ok(2));
        // a `K` column, formatted in 20 characters whatever its display width
        assert_eq!(col_str_len(5, 81, 1, 8), Ok(21));
        assert_eq!(col_str_len(30, 81, 1, 8), Ok(31));
    }

    #[test]
    fn col_str_len_refuses_what_cfitsio_cannot_hold() {
        assert_eq!(col_str_len(1, -TSTRING, 10, 1), Err(BAD_TFORM));
        assert_eq!(col_str_len(-1294967296, 41, 1, 4), Err(BAD_TFORM));
        assert_eq!(col_str_len(400, 41, 1, 4), Err(BAD_TFORM));
    }

    fn tdisp(s: &str) -> Vec<c_char> {
        s.bytes().map(|b| b as c_char).collect()
    }

    #[test]
    fn tdisp_width_limits() {
        for ok in ["", "I10", "F8.3", "EN12.4", "A20", "G399.7"] {
            assert!(tdisp_width_ok(&tdisp(ok)), "{}", ok);
        }
        for bad in ["I400", "I3000000000", "F99999.3", "A9999999999999999"] {
            assert!(!tdisp_width_ok(&tdisp(bad)), "{}", bad);
        }
    }
}
