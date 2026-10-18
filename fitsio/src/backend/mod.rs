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
/// CFITSIO datatype code of string columns (`TSTRING`)
const TSTRING: c_int = 16;

/// Number of pixels in the image region running from the 1-based `fpixel` to `lpixel`
/// inclusive, stepping by `inc` (or 1), or a CFITSIO status if the region is not valid for an
/// image with `naxis` axes.
///
/// CFITSIO reads one entry per axis from each of the arrays, so a region with too few entries
/// would make it read past their ends.
pub(crate) fn subset_len(
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

/// Size of each buffer that `fits_read_col_str` needs, including the terminating nul, or a
/// CFITSIO status if the column cannot be read this way.
///
/// CFITSIO writes a string column's full width, which can be more than its display width
/// (`TDISPn` may be narrower), and formats a 64-bit integer column in 20 characters.
/// Variable-length string columns are refused: their width varies per row.
pub(crate) fn col_str_len(
    display_width: c_int,
    typecode: c_int,
    repeat: i64,
    width: i64,
) -> Result<usize, c_int> {
    if typecode == -TSTRING {
        return Err(BAD_TFORM);
    }
    let len = i64::from(display_width).max(width);
    let len = if typecode == TSTRING {
        len.max(repeat)
    } else {
        len.max(20)
    };
    usize::try_from(len + 1).map_err(|_| BAD_TFORM)
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
        // a `20A` column displayed as `A5`
        assert_eq!(col_str_len(5, TSTRING, 20, 20), Ok(21));
        assert_eq!(col_str_len(30, TSTRING, 20, 20), Ok(31));
        assert_eq!(col_str_len(1, TSTRING, 1, 1), Ok(2));
        // a `K` column, formatted in 20 characters whatever its display width
        assert_eq!(col_str_len(5, 81, 1, 8), Ok(21));
        assert_eq!(col_str_len(1, -TSTRING, 10, 1), Err(BAD_TFORM));
    }
}
